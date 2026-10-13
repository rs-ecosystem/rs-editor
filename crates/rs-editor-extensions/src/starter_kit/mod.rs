//! The bundled extensions, mirroring Tiptap's StarterKit plus common
//! additions (underline, link, highlight, task lists, alignment, …).

mod functionality;
mod marks;
mod nodes;
mod util;

use std::sync::Arc;

pub use functionality::{
    CharacterCount, Core, CountMode, History, Placeholder, TextAlign, Typography,
};
pub use marks::{
    Bold, Code, Highlight, Italic, Link, PREVENT_AUTOLINK, Strike, Subscript, Superscript,
    Underline, detect_link,
};
pub use nodes::{
    Blockquote, BulletList, CodeBlock, Document, HardBreak, Heading, HorizontalRule, Image,
    ListItem, OrderedList, Paragraph, TaskItem, TaskList, Text, toggle_task_item,
};

use crate::extension::Extension;

/// Everything a rich text editor needs out of the box: document structure,
/// text formatting, lists, checklists, links, history and typography.
pub fn starter_kit() -> Vec<Arc<dyn Extension>> {
    vec![
        Arc::new(Document),
        Arc::new(Paragraph),
        Arc::new(Text),
        Arc::new(Heading::default()),
        Arc::new(Blockquote),
        Arc::new(CodeBlock::default()),
        Arc::new(HorizontalRule),
        Arc::new(HardBreak::default()),
        Arc::new(BulletList),
        Arc::new(OrderedList),
        Arc::new(ListItem),
        Arc::new(TaskList),
        Arc::new(TaskItem::default()),
        Arc::new(Image::default()),
        Arc::new(Link::default()),
        Arc::new(Bold),
        Arc::new(Italic),
        Arc::new(Underline),
        Arc::new(Strike),
        Arc::new(Code),
        Arc::new(Highlight::default()),
        Arc::new(Subscript),
        Arc::new(Superscript),
        Arc::new(TextAlign::default()),
        Arc::new(Typography),
        Arc::new(History::default()),
        Arc::new(Placeholder::default()),
        Arc::new(CharacterCount::default()),
        Arc::new(Core),
    ]
}
