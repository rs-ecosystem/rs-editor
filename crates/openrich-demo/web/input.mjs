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
export function preeditDocument(document, blockTexts, marks) {
  const preview = structuredClone(document);
  preview.blocks.forEach((block, index) => {
    if (!Array.isArray(block.content) || blockTexts[index] == null) return;
    const before = block.content.map(run => run.text || '').join('');
    const edit = replacementDiff(before, blockTexts[index], index);
    if (!edit) return;
    const units = block.content.flatMap(run => clusters(run.text || '').map(text => ({text, marks:run.marks || []})));
    const replacement = clusters(edit.text).map(text => ({text, marks}));
    units.splice(edit.selection.anchor.offset, edit.selection.focus.offset-edit.selection.anchor.offset, ...replacement);
    block.content = [];
    for (const unit of units) {
      const last = block.content.at(-1);
      if (last && JSON.stringify(last.marks) === JSON.stringify(unit.marks)) last.text += unit.text;
      else block.content.push({type:'text', text:unit.text, marks:unit.marks});
    }
  });
  return preview;
}

export function documentFingerprint(document) {
  return JSON.stringify(document, (_key, value) => value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, value[key]])) : value);
}
