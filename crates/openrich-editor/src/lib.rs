//! Headless editing state and commands for OpenRich.

use openrich_document::{Affinity, Block, Document, Inline, Mark, NodeId, Position, Selection};
use openrich_transaction::{Step, Transaction};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// Commands that can change the document or active formatting state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    DeleteBackward,
    DeleteForward,
    SplitBlock,
    SetBlockFormat(openrich_document::BlockFormat),
    ToggleMark(Mark),
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalDirection {
    Backward,
    Forward,
}

/// Mutable editor state, independent of any UI framework.
#[derive(Clone, Debug)]
pub struct EditorState {
    document: Document,
    selection: Selection,
    active_marks: Option<Vec<Mark>>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    typing_group_open: bool,
    composition: Option<Composition>,
    recording_insert: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composition {
    pub text: String,
    pub selection: Selection,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Debug)]
struct Snapshot {
    document: Document,
    selection: Selection,
    active_marks: Option<Vec<Mark>>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new(Document::new()).expect("the default document is valid")
    }
}

impl EditorState {
    pub fn new(document: Document) -> Result<Self, EditorError> {
        document.validate()?;
        let block_index = document
            .blocks
            .iter()
            .position(|block| block.inline_content().is_some())
            .ok_or(EditorError::NoEditableTextBlock)?;
        let selection = Selection::caret(Position {
            block_index,
            offset: 0,
            affinity: Affinity::Downstream,
        });
        Ok(Self {
            document,
            selection,
            active_marks: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            typing_group_open: false,
            composition: None,
            recording_insert: false,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Formatting marks active for text inserted at a collapsed selection.
    pub fn active_marks(&self) -> Option<&[Mark]> {
        self.active_marks.as_deref()
    }

    /// Resolved typing marks, including an explicit toolbar override.
    pub fn insertion_marks(&self) -> Vec<Mark> {
        if let Some(marks) = &self.active_marks {
            return marks.clone();
        }
        let (start, end) = ordered_positions(&self.selection);
        let Some(content) = self.document.blocks[start.block_index].inline_content() else {
            return Vec::new();
        };
        let chars = flatten_inline(content);
        let boundaries = grapheme_boundaries(&chars);
        let scalar = boundaries.get(start.offset).copied().unwrap_or(chars.len());
        chars
            .get(
                if start.block_index != end.block_index || start.offset != end.offset {
                    scalar
                } else {
                    scalar.saturating_sub(1)
                },
            )
            .or_else(|| chars.first())
            .map(|ch| ch.marks.clone())
            .unwrap_or_default()
    }

    /// Applies a native IME replacement without dropping the formatting captured
    /// when composition began. The replacement range can include reconverted text.
    pub fn replace_composition(
        &mut self,
        selection: Selection,
        text: String,
    ) -> Result<Option<Transaction>, EditorError> {
        self.validate_position(&selection.anchor)?;
        self.validate_position(&selection.focus)?;
        let Some(composition) = self.composition.take() else {
            return Ok(None);
        };
        self.selection = selection;
        self.active_marks = Some(composition.marks);
        self.dispatch(EditorCommand::InsertText(text))
    }

    /// Sets the selection after checking block types and grapheme offsets.
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), EditorError> {
        self.validate_position(&selection.anchor)?;
        self.validate_position(&selection.focus)?;
        // Affinity may change when DOM UTF-16 positions are mapped back from a
        // native keyboard. It does not mean the user moved the logical caret.
        let same_range = self.selection.anchor.block_index == selection.anchor.block_index
            && self.selection.anchor.offset == selection.anchor.offset
            && self.selection.focus.block_index == selection.focus.block_index
            && self.selection.focus.offset == selection.focus.offset;
        if !same_range {
            self.selection = selection;
            self.active_marks = None;
            self.typing_group_open = false;
            self.composition = None;
        } else {
            self.selection = selection;
        }
        Ok(())
    }

    /// Closes the current run of consecutive text insertions for undo grouping.
    pub fn end_history_group(&mut self) {
        self.typing_group_open = false;
    }

    /// Text currently being composed by an IME. It is not part of the document yet.
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    pub fn begin_composition(&mut self) {
        if self.composition.is_none() {
            self.typing_group_open = false;
            self.composition = Some(Composition {
                text: String::new(),
                selection: self.selection.clone(),
                marks: self.insertion_marks(),
            });
        }
    }

    pub fn update_composition(&mut self, text: impl Into<String>) {
        self.begin_composition();
        if let Some(composition) = &mut self.composition {
            composition.text = text.into();
        }
    }

    /// Commits the complete composition as one document edit and one undo step.
    pub fn commit_composition(
        &mut self,
        committed_text: impl Into<String>,
    ) -> Result<Option<Transaction>, EditorError> {
        let Some(composition) = self.composition.take() else {
            return Ok(None);
        };
        let committed_text = committed_text.into();
        self.selection = composition.selection;
        self.active_marks = Some(composition.marks);
        if committed_text.is_empty() {
            return Ok(None);
        }
        self.dispatch(EditorCommand::InsertText(committed_text))
    }

    pub fn cancel_composition(&mut self) -> bool {
        self.composition.take().is_some()
    }

    /// Moves by one grapheme and skips structural blocks without editable text.
    pub fn move_caret(
        &mut self,
        direction: HorizontalDirection,
        extend_selection: bool,
    ) -> Result<(), EditorError> {
        let current = self.selection.focus.clone();
        let mut next = current.clone();
        if !extend_selection && !self.selection.is_collapsed() {
            let (start, end) = ordered_positions(&self.selection);
            next = match direction {
                HorizontalDirection::Backward => start,
                HorizontalDirection::Forward => end,
            };
        } else {
            let length = self.document.blocks[current.block_index]
                .text_len()
                .ok_or(EditorError::PositionInNonTextBlock)?;
            match direction {
                HorizontalDirection::Backward if current.offset > 0 => next.offset -= 1,
                HorizontalDirection::Forward if current.offset < length => next.offset += 1,
                _ => {
                    let step: isize = if direction == HorizontalDirection::Forward {
                        1
                    } else {
                        -1
                    };
                    let mut index = current.block_index as isize + step;
                    let mut found = None;
                    while index >= 0 && (index as usize) < self.document.blocks.len() {
                        let block_index = index as usize;
                        if let Some(text_len) = self.document.blocks[block_index].text_len() {
                            found = Some(Position {
                                block_index,
                                offset: if step > 0 { 0 } else { text_len },
                                affinity: if step > 0 {
                                    Affinity::Downstream
                                } else {
                                    Affinity::Upstream
                                },
                            });
                            break;
                        }
                        index += step;
                    }
                    if let Some(position) = found {
                        next = position;
                    }
                }
            }
        }
        let selection = if extend_selection {
            Selection {
                anchor: self.selection.anchor.clone(),
                focus: next,
            }
        } else {
            Selection::caret(next)
        };
        self.set_selection(selection)
    }

    /// Returns selected plain text for a host clipboard implementation.
    pub fn selected_text(&self) -> Result<String, EditorError> {
        let (start, end) = ordered_positions(&self.selection);
        if start.block_index == end.block_index {
            let block = &self.document.blocks[start.block_index];
            let content = block
                .inline_content()
                .ok_or(EditorError::PositionInNonTextBlock)?;
            let chars = flatten_inline(content);
            let boundaries = grapheme_boundaries(&chars);
            let from = *boundaries
                .get(start.offset)
                .ok_or(EditorError::PositionOutOfBounds)?;
            let to = *boundaries
                .get(end.offset)
                .ok_or(EditorError::PositionOutOfBounds)?;
            return Ok(chars[from..to].iter().map(|ch| ch.value).collect());
        }
        let mut pieces = Vec::new();
        for index in start.block_index..=end.block_index {
            let block = &self.document.blocks[index];
            let content = block
                .inline_content()
                .ok_or(EditorError::PositionInNonTextBlock)?;
            let chars = flatten_inline(content);
            let boundaries = grapheme_boundaries(&chars);
            let from = if index == start.block_index {
                start.offset
            } else {
                0
            };
            let to = if index == end.block_index {
                end.offset
            } else {
                boundaries.len() - 1
            };
            let from = *boundaries
                .get(from)
                .ok_or(EditorError::PositionOutOfBounds)?;
            let to = *boundaries.get(to).ok_or(EditorError::PositionOutOfBounds)?;
            pieces.push(
                chars[from..to]
                    .iter()
                    .map(|ch| ch.value)
                    .collect::<String>(),
            );
        }
        Ok(pieces.join("\n"))
    }

    /// Applies a command and returns the document transaction, if one was made.
    pub fn dispatch(&mut self, command: EditorCommand) -> Result<Option<Transaction>, EditorError> {
        let is_insert = matches!(&command, EditorCommand::InsertText(_));
        if !is_insert {
            self.typing_group_open = false;
        }
        self.recording_insert = is_insert;
        let result = match command {
            EditorCommand::InsertText(text) => self.replace_selection(&text),
            EditorCommand::DeleteBackward => self.delete_backward(),
            EditorCommand::DeleteForward => self.delete_forward(),
            EditorCommand::SplitBlock => self.split_block(),
            EditorCommand::SetBlockFormat(format) => {
                let before = self.document.clone();
                let mut after = before.clone();
                let (start, end) = ordered_positions(&self.selection);
                for block in &mut after.blocks[start.block_index..=end.block_index] {
                    let content = block
                        .inline_content()
                        .ok_or(EditorError::PositionInNonTextBlock)?
                        .to_vec();
                    let id = match block {
                        Block::Paragraph { id, .. }
                        | Block::Heading { id, .. }
                        | Block::Formatted { id, .. } => id.clone(),
                        _ => unreachable!(),
                    };
                    let mut block_format = format.clone();
                    block_format.heading_level = match block {
                        Block::Heading { level, .. } => Some(*level),
                        Block::Formatted { format, .. } => format.heading_level,
                        _ => None,
                    };
                    *block = Block::Formatted {
                        id,
                        content,
                        format: block_format,
                    };
                }
                Ok(self.commit_document_change(
                    before,
                    after,
                    self.selection.clone(),
                    self.selection.clone(),
                ))
            }
            EditorCommand::ToggleMark(mark) => self.toggle_mark(mark),
            EditorCommand::Undo => Ok(self.undo()),
            EditorCommand::Redo => Ok(self.redo()),
        };
        self.recording_insert = false;
        result
    }

    fn validate_position(&self, position: &Position) -> Result<(), EditorError> {
        let block = self
            .document
            .blocks
            .get(position.block_index)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let length = block
            .text_len()
            .ok_or(EditorError::PositionInNonTextBlock)?;
        if position.offset > length {
            return Err(EditorError::PositionOutOfBounds);
        }
        Ok(())
    }

    fn replace_selection(
        &mut self,
        inserted_text: &str,
    ) -> Result<Option<Transaction>, EditorError> {
        if inserted_text.is_empty() && self.selection.is_collapsed() {
            return Ok(None);
        }
        if self.selection.anchor.block_index != self.selection.focus.block_index {
            return self.replace_cross_block_selection(inserted_text);
        }
        let (block_index, from, to) = self.selection_range()?;
        self.replace_range(block_index, from, to, inserted_text)
    }

    fn replace_cross_block_selection(
        &mut self,
        inserted_text: &str,
    ) -> Result<Option<Transaction>, EditorError> {
        let (start, end) = ordered_positions(&self.selection);
        let first = self
            .document
            .blocks
            .get(start.block_index)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let last = self
            .document
            .blocks
            .get(end.block_index)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let first_content = first
            .inline_content()
            .ok_or(EditorError::PositionInNonTextBlock)?;
        let last_content = last
            .inline_content()
            .ok_or(EditorError::PositionInNonTextBlock)?;
        let first_chars = flatten_inline(first_content);
        let last_chars = flatten_inline(last_content);
        let first_bounds = grapheme_boundaries(&first_chars);
        let last_bounds = grapheme_boundaries(&last_chars);
        let start_scalar = *first_bounds
            .get(start.offset)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let end_scalar = *last_bounds
            .get(end.offset)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let inherited_marks = self.active_marks.clone().unwrap_or_else(|| {
            first_chars
                .get(start_scalar)
                .map(|ch| ch.marks.clone())
                .or_else(|| last_chars.get(end_scalar).map(|ch| ch.marks.clone()))
                .unwrap_or_default()
        });

        let mut merged = first_chars[..start_scalar].to_vec();
        merged.extend(inserted_text.chars().map(|value| StyledChar {
            value,
            marks: inherited_marks.clone(),
        }));
        merged.extend_from_slice(&last_chars[end_scalar..]);
        let scalar_caret = start_scalar + inserted_text.chars().count();
        let caret_offset = grapheme_boundaries(&merged)
            .iter()
            .position(|boundary| *boundary >= scalar_caret)
            .unwrap_or_else(|| grapheme_boundaries(&merged).len().saturating_sub(1));

        let mut next_document = self.document.clone();
        let replacement = replace_block_inline(first, to_inline(&merged))?;
        next_document
            .blocks
            .splice(start.block_index..=end.block_index, [replacement]);
        let before_document = self.document.clone();
        let before_selection = self.selection.clone();
        let next_selection = Selection::caret(Position {
            block_index: start.block_index,
            offset: caret_offset,
            affinity: Affinity::Downstream,
        });
        Ok(self.commit_document_change(
            before_document,
            next_document,
            before_selection,
            next_selection,
        ))
    }

    fn delete_backward(&mut self) -> Result<Option<Transaction>, EditorError> {
        if !self.selection.is_collapsed() {
            return self.replace_selection("");
        }
        let position = self.selection.focus.clone();
        if position.offset == 0 {
            return self.join_with_previous(position.block_index);
        }
        self.replace_range(
            position.block_index,
            position.offset - 1,
            position.offset,
            "",
        )
    }

    fn delete_forward(&mut self) -> Result<Option<Transaction>, EditorError> {
        if !self.selection.is_collapsed() {
            return self.replace_selection("");
        }
        let position = self.selection.focus.clone();
        let length = self.document.blocks[position.block_index]
            .text_len()
            .ok_or(EditorError::PositionInNonTextBlock)?;
        if position.offset >= length {
            return self.join_with_next(position.block_index);
        }
        self.replace_range(
            position.block_index,
            position.offset,
            position.offset + 1,
            "",
        )
    }

    fn replace_range(
        &mut self,
        block_index: usize,
        from: usize,
        to: usize,
        inserted_text: &str,
    ) -> Result<Option<Transaction>, EditorError> {
        if inserted_text.is_empty() && from == to {
            return Ok(None);
        }
        let inline_content = self.document.blocks[block_index]
            .inline_content()
            .ok_or(EditorError::PositionInNonTextBlock)?
            .to_vec();
        let chars = flatten_inline(&inline_content);
        let boundaries = grapheme_boundaries(&chars);
        let start = *boundaries
            .get(from)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let end = *boundaries.get(to).ok_or(EditorError::PositionOutOfBounds)?;

        let inherited_marks = self.active_marks.clone().unwrap_or_else(|| {
            if start < end {
                chars
                    .get(start)
                    .map(|ch| ch.marks.clone())
                    .unwrap_or_default()
            } else if start > 0 {
                chars[start - 1].marks.clone()
            } else {
                chars
                    .get(end)
                    .map(|ch| ch.marks.clone())
                    .unwrap_or_default()
            }
        });

        let mut updated = Vec::with_capacity(chars.len() + inserted_text.chars().count());
        updated.extend_from_slice(&chars[..start]);
        updated.extend(inserted_text.chars().map(|value| StyledChar {
            value,
            marks: inherited_marks.clone(),
        }));
        updated.extend_from_slice(&chars[end..]);

        let next_content = to_inline(&updated);
        let next_boundaries = grapheme_boundaries(&updated);
        let caret_scalar_offset = start + inserted_text.chars().count();
        let caret_offset = next_boundaries
            .iter()
            .position(|boundary| *boundary >= caret_scalar_offset)
            .unwrap_or(next_boundaries.len() - 1);
        let next_selection = Selection::caret(Position {
            block_index,
            offset: caret_offset,
            affinity: Affinity::Downstream,
        });

        let Some(transaction) =
            self.commit_inline_change(block_index, next_content, next_selection)
        else {
            return Ok(None);
        };
        Ok(Some(transaction))
    }

    fn toggle_mark(&mut self, mark: Mark) -> Result<Option<Transaction>, EditorError> {
        if self.selection.is_collapsed() {
            let mut marks = self.active_marks.clone().unwrap_or_else(|| {
                let position = &self.selection.focus;
                self.document.blocks[position.block_index]
                    .inline_content()
                    .map(flatten_inline)
                    .map(|chars| {
                        let boundaries = grapheme_boundaries(&chars);
                        let scalar = boundaries[position.offset];
                        if scalar > 0 {
                            chars[scalar - 1].marks.clone()
                        } else {
                            chars.first().map(|ch| ch.marks.clone()).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default()
            });
            if let Some(index) = marks.iter().position(|active| active == &mark) {
                marks.remove(index);
            } else {
                marks.push(mark);
            }
            self.active_marks = Some(marks);
            return Ok(None);
        }

        if self.selection.anchor.block_index != self.selection.focus.block_index {
            return self.toggle_mark_across_blocks(mark);
        }
        let (block_index, from, to) = self.selection_range()?;
        let inline_content = self.document.blocks[block_index]
            .inline_content()
            .ok_or(EditorError::PositionInNonTextBlock)?
            .to_vec();
        let mut chars = flatten_inline(&inline_content);
        let boundaries = grapheme_boundaries(&chars);
        let start = *boundaries
            .get(from)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let end = *boundaries.get(to).ok_or(EditorError::PositionOutOfBounds)?;
        if start == end {
            return Ok(None);
        }
        let should_remove = chars[start..end]
            .iter()
            .all(|ch| ch.marks.iter().any(|existing| existing == &mark));
        for ch in &mut chars[start..end] {
            if should_remove {
                ch.marks.retain(|existing| existing != &mark);
            } else if !ch.marks.iter().any(|existing| existing == &mark) {
                ch.marks.push(mark.clone());
            }
        }
        let next_content = to_inline(&chars);
        let next_selection = self.selection.clone();
        Ok(self.commit_inline_change(block_index, next_content, next_selection))
    }

    fn toggle_mark_across_blocks(
        &mut self,
        mark: Mark,
    ) -> Result<Option<Transaction>, EditorError> {
        let (start, end) = ordered_positions(&self.selection);
        let mut next_document = self.document.clone();
        let mut changed = false;
        for index in start.block_index..=end.block_index {
            let block = &self.document.blocks[index];
            let content = block
                .inline_content()
                .ok_or(EditorError::PositionInNonTextBlock)?;
            let mut chars = flatten_inline(content);
            let boundaries = grapheme_boundaries(&chars);
            let from = if index == start.block_index {
                start.offset
            } else {
                0
            };
            let to = if index == end.block_index {
                end.offset
            } else {
                boundaries.len() - 1
            };
            let scalar_start = *boundaries
                .get(from)
                .ok_or(EditorError::PositionOutOfBounds)?;
            let scalar_end = *boundaries.get(to).ok_or(EditorError::PositionOutOfBounds)?;
            if scalar_start == scalar_end {
                continue;
            }
            let remove = chars[scalar_start..scalar_end]
                .iter()
                .all(|ch| ch.marks.iter().any(|existing| existing == &mark));
            for ch in &mut chars[scalar_start..scalar_end] {
                if remove {
                    ch.marks.retain(|existing| existing != &mark);
                } else if !ch.marks.iter().any(|existing| existing == &mark) {
                    ch.marks.push(mark.clone());
                }
            }
            *next_document.blocks[index]
                .inline_content_mut()
                .expect("validated text block") = to_inline(&chars);
            changed = true;
        }
        if !changed {
            return Ok(None);
        }
        let before_document = self.document.clone();
        let before_selection = self.selection.clone();
        Ok(self.commit_document_change(
            before_document,
            next_document,
            before_selection.clone(),
            before_selection,
        ))
    }

    fn split_block(&mut self) -> Result<Option<Transaction>, EditorError> {
        let (block_index, from, to) = self.selection_range()?;
        if let Block::Formatted { format, .. } = &self.document.blocks[block_index] {
            if self.document.blocks[block_index].text_len() == Some(0)
                && format.list != openrich_document::ListStyle::None
            {
                let mut format = format.clone();
                format.list = openrich_document::ListStyle::None;
                format.checked = false;
                return self.dispatch(EditorCommand::SetBlockFormat(format));
            }
        }
        let block = self
            .document
            .blocks
            .get(block_index)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let chars = flatten_inline(
            block
                .inline_content()
                .ok_or(EditorError::PositionInNonTextBlock)?,
        );
        let boundaries = grapheme_boundaries(&chars);
        let start = *boundaries
            .get(from)
            .ok_or(EditorError::PositionOutOfBounds)?;
        let end = *boundaries.get(to).ok_or(EditorError::PositionOutOfBounds)?;
        let left_content = to_inline(&chars[..start]);
        let right_content = to_inline(&chars[end..]);
        let (left, right) = match block {
            Block::Formatted { id, format, .. } => {
                let mut next_format = format.clone();
                next_format.checked = false;
                if block.text_len() == Some(0) {
                    next_format.list = openrich_document::ListStyle::None;
                }
                (
                    Block::Formatted {
                        id: id.clone(),
                        content: left_content,
                        format: format.clone(),
                    },
                    Block::Formatted {
                        id: NodeId::new(),
                        content: right_content,
                        format: next_format,
                    },
                )
            }
            Block::Paragraph { id, .. } => (
                Block::Paragraph {
                    id: id.clone(),
                    content: left_content,
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: right_content,
                },
            ),
            Block::Heading { id, level, .. } => (
                Block::Heading {
                    id: id.clone(),
                    level: *level,
                    content: left_content,
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: right_content,
                },
            ),
            _ => return Err(EditorError::PositionInNonTextBlock),
        };

        let before_selection = self.selection.clone();
        let before_document = self.document.clone();
        let mut next_document = before_document.clone();
        drop(
            next_document
                .blocks
                .splice(block_index..=block_index, [left, right]),
        );
        let next_selection = Selection::caret(Position {
            block_index: block_index + 1,
            offset: 0,
            affinity: Affinity::Downstream,
        });
        Ok(self.commit_document_change(
            before_document,
            next_document,
            before_selection,
            next_selection,
        ))
    }

    fn join_with_previous(
        &mut self,
        block_index: usize,
    ) -> Result<Option<Transaction>, EditorError> {
        if block_index == 0 {
            return Ok(None);
        }
        let previous = &self.document.blocks[block_index - 1];
        let current = &self.document.blocks[block_index];
        let (Some(previous_content), Some(current_content)) =
            (previous.inline_content(), current.inline_content())
        else {
            return Ok(None);
        };
        let caret_offset = previous.text_len().unwrap_or_default();
        let mut joined = previous_content.to_vec();
        joined.extend_from_slice(current_content);
        let previous_block = match previous {
            Block::Formatted { id, format, .. } => Block::Formatted {
                id: id.clone(),
                content: joined.clone(),
                format: format.clone(),
            },
            Block::Paragraph { id, .. } => Block::Paragraph {
                id: id.clone(),
                content: joined,
            },
            Block::Heading { id, level, .. } => Block::Heading {
                id: id.clone(),
                level: *level,
                content: joined,
            },
            _ => return Ok(None),
        };
        let before_selection = self.selection.clone();
        let before_document = self.document.clone();
        let mut next_document = before_document.clone();
        drop(
            next_document
                .blocks
                .splice(block_index - 1..=block_index, [previous_block]),
        );
        let next_selection = Selection::caret(Position {
            block_index: block_index - 1,
            offset: caret_offset,
            affinity: Affinity::Downstream,
        });
        Ok(self.commit_document_change(
            before_document,
            next_document,
            before_selection,
            next_selection,
        ))
    }

    fn join_with_next(&mut self, block_index: usize) -> Result<Option<Transaction>, EditorError> {
        if block_index + 1 >= self.document.blocks.len() {
            return Ok(None);
        }
        let current = &self.document.blocks[block_index];
        let next = &self.document.blocks[block_index + 1];
        let (Some(current_content), Some(next_content)) =
            (current.inline_content(), next.inline_content())
        else {
            return Ok(None);
        };
        let mut joined = current_content.to_vec();
        joined.extend_from_slice(next_content);
        let current_block = match current {
            Block::Formatted { id, format, .. } => Block::Formatted {
                id: id.clone(),
                content: joined.clone(),
                format: format.clone(),
            },
            Block::Paragraph { id, .. } => Block::Paragraph {
                id: id.clone(),
                content: joined,
            },
            Block::Heading { id, level, .. } => Block::Heading {
                id: id.clone(),
                level: *level,
                content: joined,
            },
            _ => return Ok(None),
        };
        let caret_offset = current.text_len().unwrap_or_default();
        let before_selection = self.selection.clone();
        let before_document = self.document.clone();
        let mut next_document = before_document.clone();
        drop(
            next_document
                .blocks
                .splice(block_index..=block_index + 1, [current_block]),
        );
        let next_selection = Selection::caret(Position {
            block_index,
            offset: caret_offset,
            affinity: Affinity::Downstream,
        });
        Ok(self.commit_document_change(
            before_document,
            next_document,
            before_selection,
            next_selection,
        ))
    }

    fn selection_range(&self) -> Result<(usize, usize, usize), EditorError> {
        let anchor = &self.selection.anchor;
        let focus = &self.selection.focus;
        if anchor.block_index != focus.block_index {
            return Err(EditorError::CrossBlockSelectionUnsupported);
        }
        let (from, to) = if anchor.offset <= focus.offset {
            (anchor.offset, focus.offset)
        } else {
            (focus.offset, anchor.offset)
        };
        Ok((anchor.block_index, from, to))
    }

    fn commit_inline_change(
        &mut self,
        block_index: usize,
        next_content: Vec<Inline>,
        next_selection: Selection,
    ) -> Option<Transaction> {
        let before_content = self.document.blocks[block_index]
            .inline_content()
            .expect("commit is only called for text blocks")
            .to_vec();
        let before_selection = self.selection.clone();
        if before_content == next_content {
            self.selection = next_selection;
            return None;
        }

        let group_typing = self.current_dispatch_is_insert();
        if !group_typing || !self.typing_group_open {
            let snapshot = self.snapshot();
            self.undo_stack.push(snapshot);
        }
        if group_typing {
            self.typing_group_open = true;
        }
        self.redo_stack.clear();
        *self.document.blocks[block_index]
            .inline_content_mut()
            .expect("commit is only called for text blocks") = next_content.clone();
        self.selection = next_selection.clone();
        Some(Transaction::new(
            vec![Step::ReplaceInlineContent {
                block_index,
                before: before_content,
                after: next_content,
            }],
            before_selection,
            next_selection,
        ))
    }

    fn current_dispatch_is_insert(&self) -> bool {
        self.recording_insert
    }

    fn commit_document_change(
        &mut self,
        before_document: Document,
        next_document: Document,
        before_selection: Selection,
        next_selection: Selection,
    ) -> Option<Transaction> {
        if before_document == next_document {
            return None;
        }
        let snapshot = self.snapshot();
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
        self.document = next_document.clone();
        self.selection = next_selection.clone();
        self.typing_group_open = false;
        self.active_marks = None;
        Some(Transaction::new(
            vec![Step::ReplaceDocument {
                before: before_document,
                after: next_document,
            }],
            before_selection,
            next_selection,
        ))
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            document: self.document.clone(),
            selection: self.selection.clone(),
            active_marks: self.active_marks.clone(),
        }
    }

    fn undo(&mut self) -> Option<Transaction> {
        let target = self.undo_stack.pop()?;
        let before_document = self.document.clone();
        let before_selection = self.selection.clone();
        let snapshot = self.snapshot();
        self.redo_stack.push(snapshot);
        self.document = target.document;
        self.selection = target.selection;
        self.active_marks = target.active_marks;
        Some(Transaction::new(
            vec![Step::ReplaceDocument {
                before: before_document,
                after: self.document.clone(),
            }],
            before_selection,
            self.selection.clone(),
        ))
    }

    fn redo(&mut self) -> Option<Transaction> {
        let target = self.redo_stack.pop()?;
        let before_document = self.document.clone();
        let before_selection = self.selection.clone();
        let snapshot = self.snapshot();
        self.undo_stack.push(snapshot);
        self.document = target.document;
        self.selection = target.selection;
        self.active_marks = target.active_marks;
        Some(Transaction::new(
            vec![Step::ReplaceDocument {
                before: before_document,
                after: self.document.clone(),
            }],
            before_selection,
            self.selection.clone(),
        ))
    }
}

fn ordered_positions(selection: &Selection) -> (Position, Position) {
    let anchor = &selection.anchor;
    let focus = &selection.focus;
    if (anchor.block_index, anchor.offset) <= (focus.block_index, focus.offset) {
        (anchor.clone(), focus.clone())
    } else {
        (focus.clone(), anchor.clone())
    }
}

fn replace_block_inline(block: &Block, content: Vec<Inline>) -> Result<Block, EditorError> {
    match block {
        Block::Formatted { id, format, .. } => Ok(Block::Formatted {
            id: id.clone(),
            content,
            format: format.clone(),
        }),
        Block::Paragraph { id, .. } => Ok(Block::Paragraph {
            id: id.clone(),
            content,
        }),
        Block::Heading { id, level, .. } => Ok(Block::Heading {
            id: id.clone(),
            level: *level,
            content,
        }),
        _ => Err(EditorError::PositionInNonTextBlock),
    }
}

#[derive(Clone, Debug)]
struct StyledChar {
    value: char,
    marks: Vec<Mark>,
}

fn flatten_inline(content: &[Inline]) -> Vec<StyledChar> {
    content
        .iter()
        .flat_map(|inline| match inline {
            Inline::Text { text, marks } => text
                .chars()
                .map(|value| StyledChar {
                    value,
                    marks: marks.clone(),
                })
                .collect::<Vec<_>>(),
        })
        .collect()
}

fn grapheme_boundaries(chars: &[StyledChar]) -> Vec<usize> {
    let text = chars.iter().map(|ch| ch.value).collect::<String>();
    let mut boundaries = Vec::with_capacity(text.graphemes(true).count() + 1);
    let mut scalar_offset = 0;
    boundaries.push(scalar_offset);
    for grapheme in text.graphemes(true) {
        scalar_offset += grapheme.chars().count();
        boundaries.push(scalar_offset);
    }
    boundaries
}

fn to_inline(chars: &[StyledChar]) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut current_text = String::new();
    let mut current_marks: Option<Vec<Mark>> = None;

    for ch in chars {
        if current_marks
            .as_ref()
            .is_some_and(|marks| marks != &ch.marks)
        {
            result.push(Inline::Text {
                text: std::mem::take(&mut current_text),
                marks: current_marks.take().expect("marks are present"),
            });
        }
        current_marks.get_or_insert_with(|| ch.marks.clone());
        current_text.push(ch.value);
    }

    if let Some(marks) = current_marks {
        result.push(Inline::Text {
            text: current_text,
            marks,
        });
    }
    result
}

#[derive(Debug, Error)]
pub enum EditorError {
    #[error(transparent)]
    Document(#[from] openrich_document::DocumentError),
    #[error("selection endpoint refers to a block outside the document")]
    PositionOutOfBounds,
    #[error("selection endpoint must refer to a text-bearing block")]
    PositionInNonTextBlock,
    #[error("document has no text-bearing block for an initial caret")]
    NoEditableTextBlock,
    #[error("editing a selection across multiple blocks is not supported yet")]
    CrossBlockSelectionUnsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_in(block: &Block) -> String {
        block
            .inline_content()
            .unwrap()
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, .. } => text.as_str(),
            })
            .collect()
    }

    fn caret(offset: usize) -> Selection {
        Selection::caret(Position {
            block_index: 0,
            offset,
            affinity: Affinity::Downstream,
        })
    }

    #[test]
    fn edits_and_deletes_whole_graphemes_and_supports_undo_redo() {
        let document = Document {
            schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Paragraph {
                id: NodeId::new(),
                content: vec![Inline::text("A🙂B")],
            }],
        };
        let mut editor = EditorState::new(document).unwrap();
        editor.set_selection(caret(2)).unwrap();

        let inserted = editor
            .dispatch(EditorCommand::InsertText("x".into()))
            .unwrap();
        assert!(inserted.is_some());
        assert_eq!(text_in(&editor.document().blocks[0]), "A🙂xB");
        assert_eq!(editor.selection().focus.offset, 3);

        editor.dispatch(EditorCommand::DeleteBackward).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "A🙂B");

        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "A🙂xB");

        editor.dispatch(EditorCommand::Redo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "A🙂B");
    }

    #[test]
    fn active_mark_is_applied_to_inserted_text() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor
            .dispatch(EditorCommand::InsertText("bold".into()))
            .unwrap();

        assert_eq!(
            editor.document().blocks[0].inline_content().unwrap(),
            &[Inline::Text {
                text: "bold".into(),
                marks: vec![Mark::Bold],
            }]
        );
    }

    #[test]
    fn splitting_and_joining_blocks_round_trip_through_history() {
        let document = Document {
            schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Paragraph {
                id: NodeId::new(),
                content: vec![Inline::text("hello")],
            }],
        };
        let mut editor = EditorState::new(document).unwrap();
        editor.set_selection(caret(2)).unwrap();
        editor.dispatch(EditorCommand::SplitBlock).unwrap();

        assert_eq!(editor.document().blocks.len(), 2);
        assert_eq!(text_in(&editor.document().blocks[0]), "he");
        assert_eq!(text_in(&editor.document().blocks[1]), "llo");

        editor.dispatch(EditorCommand::DeleteBackward).unwrap();
        assert_eq!(editor.document().blocks.len(), 1);
        assert_eq!(text_in(&editor.document().blocks[0]), "hello");

        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(editor.document().blocks.len(), 2);
    }

    #[test]
    fn split_preserves_original_node_id_on_left_block() {
        let mut editor = EditorState::default();
        let original_id = match &editor.document().blocks[0] {
            Block::Paragraph { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        editor
            .dispatch(EditorCommand::InsertText("ab".into()))
            .unwrap();
        editor.set_selection(caret(1)).unwrap();
        editor.dispatch(EditorCommand::SplitBlock).unwrap();

        assert!(
            matches!(&editor.document().blocks[0], Block::Paragraph { id, .. } if *id == original_id)
        );
        assert!(
            matches!(&editor.document().blocks[1], Block::Paragraph { id, .. } if *id != original_id)
        );
    }

    #[test]
    fn replaces_and_formats_a_selection_across_text_blocks() {
        let document = Document {
            schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("hello")],
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("world")],
                },
            ],
        };
        let mut editor = EditorState::new(document).unwrap();
        editor
            .set_selection(Selection {
                anchor: Position {
                    block_index: 0,
                    offset: 2,
                    affinity: Affinity::Downstream,
                },
                focus: Position {
                    block_index: 1,
                    offset: 3,
                    affinity: Affinity::Downstream,
                },
            })
            .unwrap();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        assert_eq!(editor.document().blocks.len(), 2);
        assert!(editor.document().blocks[0].inline_content().unwrap().iter().any(
            |inline| matches!(inline, Inline::Text { text, marks } if text == "llo" && marks == &[Mark::Bold])
        ));
        assert!(editor.document().blocks[1].inline_content().unwrap().iter().any(
            |inline| matches!(inline, Inline::Text { text, marks } if text == "wor" && marks == &[Mark::Bold])
        ));

        editor
            .dispatch(EditorCommand::InsertText("!".into()))
            .unwrap();
        assert_eq!(editor.document().blocks.len(), 1);
        assert_eq!(text_in(&editor.document().blocks[0]), "he!ld");
    }

    #[test]
    fn consecutive_insertions_share_an_undo_step() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::InsertText("o".into()))
            .unwrap();
        editor
            .dispatch(EditorCommand::InsertText("k".into()))
            .unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "ok");
        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "");
        editor.dispatch(EditorCommand::Redo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "ok");
    }

    #[test]
    fn ime_preedit_is_ephemeral_and_commit_is_one_undo_step() {
        let mut editor = EditorState::default();
        editor.begin_composition();
        editor.update_composition("に");
        editor.update_composition("日本語");
        assert_eq!(editor.composition().unwrap().text, "日本語");
        assert_eq!(text_in(&editor.document().blocks[0]), "");
        editor.commit_composition("日本語").unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "日本語");
        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "");

        editor.begin_composition();
        editor.update_composition("discard me");
        assert!(editor.cancel_composition());
        assert!(editor.composition().is_none());
    }

    #[test]
    fn selected_text_returns_plain_text_across_blocks() {
        let document = Document {
            schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("first")],
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("second")],
                },
            ],
        };
        let mut editor = EditorState::new(document).unwrap();
        editor
            .set_selection(Selection {
                anchor: Position {
                    block_index: 0,
                    offset: 2,
                    affinity: Affinity::Downstream,
                },
                focus: Position {
                    block_index: 1,
                    offset: 3,
                    affinity: Affinity::Downstream,
                },
            })
            .unwrap();
        assert_eq!(editor.selected_text().unwrap(), "rst\nsec");
    }

    #[test]
    fn horizontal_navigation_skips_non_text_blocks_and_extends_selection() {
        let document = Document {
            schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("a")],
                },
                Block::HorizontalRule { id: NodeId::new() },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("bc")],
                },
            ],
        };
        let mut editor = EditorState::new(document).unwrap();
        editor
            .move_caret(HorizontalDirection::Forward, false)
            .unwrap();
        editor
            .move_caret(HorizontalDirection::Forward, false)
            .unwrap();
        assert_eq!(editor.selection().focus.block_index, 2);
        assert_eq!(editor.selection().focus.offset, 0);
        editor
            .move_caret(HorizontalDirection::Forward, true)
            .unwrap();
        assert_eq!(editor.selected_text().unwrap(), "b");
    }

    #[test]
    fn unicode_edit_transactions_keep_external_document_in_sync() {
        let corpus = ["", "a🙂", "e\u{301}", "👩‍💻", "עברית", "العربية", "हिन्दी"];
        for initial in corpus {
            let original = Document {
                schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
                blocks: vec![Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text(initial)],
                }],
            };
            let mut host_document = original.clone();
            let mut editor = EditorState::new(original.clone()).unwrap();
            editor
                .set_selection(caret(initial.graphemes(true).count()))
                .unwrap();
            let transaction = editor
                .dispatch(EditorCommand::InsertText("🙂".into()))
                .unwrap()
                .unwrap();
            transaction.apply(&mut host_document).unwrap();
            assert_eq!(&host_document, editor.document());
            assert!(host_document.validate().is_ok());

            let undo = editor.dispatch(EditorCommand::Undo).unwrap().unwrap();
            undo.apply(&mut host_document).unwrap();
            assert_eq!(&host_document, &original);
            assert_eq!(text_in(&host_document.blocks[0]), initial);
        }
    }
    #[test]
    fn affinity_only_change_preserves_typing_marks_and_composition() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor.begin_composition();
        let mut selection = editor.selection().clone();
        selection.anchor.affinity = Affinity::Upstream;
        selection.focus.affinity = Affinity::Upstream;
        editor.set_selection(selection).unwrap();
        assert_eq!(editor.active_marks(), Some([Mark::Bold].as_slice()));
        assert!(editor.composition().is_some());
        editor.commit_composition("a").unwrap();
        assert_eq!(
            editor.document().blocks[0].inline_content().unwrap(),
            &[Inline::Text {
                text: "a".into(),
                marks: vec![Mark::Bold]
            }]
        );
    }

    #[test]
    fn mobile_reconversion_preserves_explicit_bold_override_and_undo() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::InsertText("cat".into()))
            .unwrap();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor.begin_composition();
        let range = Selection {
            anchor: caret(0).anchor,
            focus: caret(3).focus,
        };
        editor.replace_composition(range, "dog".into()).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "dog");
        assert_eq!(
            editor.document().blocks[0].inline_content().unwrap(),
            &[Inline::Text {
                text: "dog".into(),
                marks: vec![Mark::Bold]
            }]
        );
        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "cat");
        editor.dispatch(EditorCommand::Redo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "dog");
    }

    #[test]
    fn replacing_selected_bold_inherits_selected_text_in_both_directions() {
        for prefix in ["", "a"] {
            for reversed in [false, true] {
                for composition in [false, true] {
                    let document = Document {
                        schema_version: openrich_document::DOCUMENT_SCHEMA_VERSION,
                        blocks: vec![Block::Paragraph {
                            id: NodeId::new(),
                            content: vec![
                                Inline::text(prefix),
                                Inline::Text {
                                    text: "B".into(),
                                    marks: vec![Mark::Bold],
                                },
                                Inline::text("c"),
                            ],
                        }],
                    };
                    let mut editor = EditorState::new(document).unwrap();
                    let start = prefix.len();
                    let selection = Selection {
                        anchor: caret(if reversed { start + 1 } else { start }).anchor,
                        focus: caret(if reversed { start } else { start + 1 }).focus,
                    };
                    editor.set_selection(selection.clone()).unwrap();
                    assert_eq!(editor.insertion_marks(), vec![Mark::Bold]);
                    if composition {
                        editor.begin_composition();
                        editor.replace_composition(selection, "X".into()).unwrap();
                    } else {
                        editor
                            .dispatch(EditorCommand::InsertText("X".into()))
                            .unwrap();
                    }
                    let content = editor.document().blocks[0].inline_content().unwrap();
                    assert!(content.iter().any(|run| matches!(run, Inline::Text { text, marks } if text == "X" && marks == &[Mark::Bold])));
                }
            }
        }
    }

    #[test]
    fn mobile_reconversion_preserves_explicit_bold_off() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor
            .dispatch(EditorCommand::InsertText("cat".into()))
            .unwrap();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor.begin_composition();
        editor
            .replace_composition(
                Selection {
                    anchor: caret(0).anchor,
                    focus: caret(3).focus,
                },
                "dog".into(),
            )
            .unwrap();
        assert_eq!(
            editor.document().blocks[0].inline_content().unwrap(),
            &[Inline::Text {
                text: "dog".into(),
                marks: vec![]
            }]
        );
    }

    #[test]
    fn cancelled_and_duplicate_composition_do_not_duplicate_document() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::ToggleMark(Mark::Bold))
            .unwrap();
        editor.begin_composition();
        editor.cancel_composition();
        editor
            .replace_composition(caret(0), "ignored".into())
            .unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "");
        assert_eq!(editor.active_marks(), Some([Mark::Bold].as_slice()));
        editor.begin_composition();
        editor.replace_composition(caret(0), "👩‍💻".into()).unwrap();
        editor.replace_composition(caret(0), "👩‍💻".into()).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "👩‍💻");
        assert_eq!(editor.selection().focus.offset, 1);
    }

    #[test]
    fn composition_can_delete_a_reconverted_range() {
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::InsertText("a🙂b".into()))
            .unwrap();
        editor.begin_composition();
        editor
            .replace_composition(
                Selection {
                    anchor: caret(1).anchor,
                    focus: caret(2).focus,
                },
                "".into(),
            )
            .unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "ab");
        editor.dispatch(EditorCommand::Undo).unwrap();
        assert_eq!(text_in(&editor.document().blocks[0]), "a🙂b");
    }

    #[test]
    fn checklist_continues_unchecked_and_empty_enter_exits_list() {
        use openrich_document::{BlockFormat, ListStyle, TextAlignment};
        let mut editor = EditorState::default();
        editor
            .dispatch(EditorCommand::SetBlockFormat(BlockFormat {
                list: ListStyle::Checkbox,
                checked: true,
                alignment: TextAlignment::Right,
                ..Default::default()
            }))
            .unwrap();
        editor
            .dispatch(EditorCommand::InsertText("item".into()))
            .unwrap();
        editor.dispatch(EditorCommand::SplitBlock).unwrap();
        assert!(
            matches!(&editor.document().blocks[1], Block::Formatted {format, ..} if !format.checked && format.list == ListStyle::Checkbox && format.alignment == TextAlignment::Right)
        );
        editor.dispatch(EditorCommand::SplitBlock).unwrap();
        assert_eq!(editor.document().blocks.len(), 2);
        assert!(
            matches!(&editor.document().blocks[1], Block::Formatted {format, ..} if format.list == ListStyle::None)
        );
        editor.dispatch(EditorCommand::Undo).unwrap();
        assert!(
            matches!(&editor.document().blocks[1], Block::Formatted {format, ..} if format.list == ListStyle::Checkbox)
        );
    }
}
