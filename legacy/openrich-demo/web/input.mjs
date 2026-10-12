// Pure policies shared by the browser input adapter and regression tests.
const segmenter = new Intl.Segmenter(undefined, {granularity:'grapheme'});
const clusters = text => [...segmenter.segment(text)].map(part => part.segment);
export function replacementDiff(beforeText, afterText, block_index = 0) {
  const before = clusters(beforeText), after = clusters(afterText);
  let prefix = 0, suffix = 0;
  while (prefix < before.length && prefix < after.length && before[prefix] === after[prefix]) prefix++;
  while (suffix < before.length - prefix && suffix < after.length - prefix && before.at(-1 - suffix) === after.at(-1 - suffix)) suffix++;
  if (prefix === before.length && prefix === after.length) return null;
  const anchor = {block_index, offset:prefix, affinity:'downstream'};
  return {text:after.slice(prefix, after.length - suffix).join(''), selection:{anchor, focus:{...anchor, offset:before.length - suffix}}};
}
function orderedEndpoints(selection) {
  const {anchor:a, focus:f} = selection;
  return a.block_index < f.block_index || (a.block_index === f.block_index && a.offset <= f.offset) ? [a, f] : [f, a];
}
// Maps the native DOM after an IME composition to engine replacements.
// `snapshot[i]` is the text of block i when composition began, or null for
// structural blocks (lists, quotes, rules) that the IME cannot edit directly.
// `domTexts[i]` is the block's current DOM text, or null when the browser
// removed its element. Composing over a selection that spans blocks makes the
// browser merge them into the first block; that becomes one cross-block edit.
// `staleCaret` is the DOM caret when composition began while typed text was
// still in flight to the engine. The browser composes there, behind the
// logical caret `selection`; an insertion at the stale caret is moved forward.
export function compositionEdits(snapshot, domTexts, selection, staleCaret = null) {
  const [start, end] = selection ? orderedEndpoints(selection) : [null, null];
  if (staleCaret && start && start.block_index === end.block_index && start.offset === end.offset) {
    const block = staleCaret.block_index;
    const before = clusters(snapshot[block] ?? '');
    const prefix = before.slice(0, staleCaret.offset).join(''), suffix = before.slice(staleCaret.offset).join('');
    const after = domTexts[block];
    const others = snapshot.every((text, index) => text == null || index === block || domTexts[index] === text);
    if (snapshot[block] != null && after != null && others && after.length >= prefix.length + suffix.length && after.startsWith(prefix) && after.endsWith(suffix)) {
      const inserted = after.slice(prefix.length, after.length - suffix.length);
      return {edits:inserted ? [{text:inserted, selection:{anchor:{...start}, focus:{...start}}}] : [], unsupported:false};
    }
  }
  if (start && start.block_index !== end.block_index) {
    const unchanged = snapshot.every((text, index) => text == null || domTexts[index] === text);
    if (unchanged) return {edits:[], unsupported:false};
    const merged = domTexts[start.block_index];
    const prefix = clusters(snapshot[start.block_index] ?? '').slice(0, start.offset).join('');
    const suffix = clusters(snapshot[end.block_index] ?? '').slice(end.offset).join('');
    const removed = domTexts.slice(start.block_index + 1, end.block_index + 1);
    const untouched = snapshot.every((text, index) => text == null || (index >= start.block_index && index <= end.block_index) || domTexts[index] === text);
    if (snapshot[start.block_index] == null || snapshot[end.block_index] == null || merged == null || removed.some(text => text != null) || !untouched ||
        merged.length < prefix.length + suffix.length || !merged.startsWith(prefix) || !merged.endsWith(suffix)) return {edits:[], unsupported:true};
    return {edits:[{text:merged.slice(prefix.length, merged.length - suffix.length), selection:{anchor:start, focus:end}}], unsupported:false};
  }
  const edits = [];
  let unsupported = false;
  snapshot.forEach((before, index) => {
    if (before == null) return;
    if (domTexts[index] == null) { unsupported = true; return; }
    const edit = replacementDiff(before, domTexts[index], index);
    if (edit) edits.push(edit);
  });
  return unsupported || edits.length > 1 ? {edits:[], unsupported:true} : {edits, unsupported:false};
}
export function typingMarks(state, selection = state?.selection) {
  if (state?.active_marks != null) return state.active_marks;
  const a = selection?.anchor, f = selection?.focus;
  if (!a || !f) return [];
  const start = a.block_index < f.block_index || (a.block_index === f.block_index && a.offset <= f.offset) ? a : f;
  const block = state.document.blocks[start.block_index];
  const selected = a.block_index !== f.block_index || a.offset !== f.offset;
  const target = start.offset + (selected ? 1 : 0);
  let count = 0;
  for (const run of block?.content || []) {
    count += clusters(run.text || '').length;
    if (count >= Math.max(1, target)) return run.marks || [];
  }
  return [];
}
export function typingStyle(marks, headingLevel) {
  const has = type => marks.some(mark => mark.type === type);
  return {
    fontWeight: has('bold') || headingLevel ? '700' : '400',
    fontStyle: has('italic') ? 'italic' : 'normal',
    textDecoration: [has('underline') ? 'underline' : '', has('strike') ? 'line-through' : ''].filter(Boolean).join(' ') || 'none',
    fontFamily: has('code') ? '"OpenRich Mono",monospace' : '"OpenRich Sans",sans-serif',
    fontSize: `${headingLevel ? ({1:26.4,2:22.4,3:19.2}[headingLevel] || 17.28) : 16}px`,
  };
}
const sceneKeys = new WeakMap();
export function sceneMatches(scene, documentKey) {
  if (!scene) return false;
  if (!sceneKeys.has(scene)) sceneKeys.set(scene, JSON.stringify(scene.document));
  return sceneKeys.get(scene) === documentKey;
}
export function wordRange(text, utf16Offset, affinity = 'downstream') {
  if (!text) return null;
  const offset = Math.max(0,Math.min(text.length - 1, utf16Offset - (affinity === 'upstream' ? 1 : 0)));
  const words = new Intl.Segmenter(undefined,{granularity:'word'});
  const segment = [...words.segment(text)].find(part => offset >= part.index && offset < part.index + part.segment.length);
  return segment ? {start:segment.index,end:segment.index+segment.segment.length} : null;
}
export function touchMovement(session, x, y) {
  if (session.selecting) return 'selection';
  if (session.moved || Math.hypot(x-session.x,y-session.y)>10) return 'scroll';
  return 'pending';
}
export function isDoubleTap(previous, session) {
  return !!previous && session.time-previous.time>=0 && session.time-previous.time<350 && Math.hypot(session.x-previous.x,session.y-previous.y)<24;
}

export function selectionOwner(composing) { return 'skia'; }

// Render IME text through Skia without committing it or replacing live DOM nodes.
export function blockTexts(document) {
  return document.blocks.map(block => Array.isArray(block.content) ? block.content.map(run => run.text || '').join('') : null);
}
// Applies replacements in document coordinates; returns null when an edit does
// not fit the document (it still lacks text the edits were computed against).
export function applyEdits(document, edits, marks) {
  const result = structuredClone(document);
  const units = block => block.content.flatMap(run => clusters(run.text || '').map(text => ({text, marks:run.marks || []})));
  for (const edit of edits) {
    const {anchor:start, focus:end} = edit.selection;
    const first = result.blocks[start.block_index], last = result.blocks[end.block_index];
    if (!Array.isArray(first?.content) || !Array.isArray(last?.content) || start.offset > units(first).length || end.offset > units(last).length) return null;
    const replacement = clusters(edit.text).map(text => ({text, marks}));
    const merged = [...units(first).slice(0, start.offset), ...replacement, ...units(last).slice(end.offset)];
    first.content = [];
    for (const unit of merged) {
      const previous = first.content.at(-1);
      if (previous && JSON.stringify(previous.marks) === JSON.stringify(unit.marks)) previous.text += unit.text;
      else first.content.push({type:'text', text:unit.text, marks:unit.marks});
    }
    result.blocks.splice(start.block_index + 1, end.block_index - start.block_index);
  }
  return result;
}
// Preedit for Skia. Edits are computed against the composition snapshot and,
// for a stale caret, placed in `target`, the engine's latest document.
export function preeditDocument(document, domTexts, marks, selection = null, staleCaret = null, target = document) {
  const {edits} = compositionEdits(blockTexts(document), domTexts, selection, staleCaret);
  return applyEdits(staleCaret ? target : document, edits, marks) ?? structuredClone(target);
}

export function documentFingerprint(document) {
  return JSON.stringify(document, (_key, value) => value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, value[key]])) : value);
}
