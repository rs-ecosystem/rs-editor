import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareScene, hitTest, caretRect, selectionRects, navigate } from '../web/interaction.mjs';
const glyph = (offset, x, y, w = 10, rtl = false) => ({ offset, x, y, w, h: 18,
  left: rtl ? offset + 1 : offset, right: rtl ? offset : offset + 1,
  left_affinity: rtl ? 'upstream' : 'downstream', right_affinity: rtl ? 'downstream' : 'upstream' });
const scene = prepareScene({ width: 100, height: 100, blocks: [
  { index: 0, glyphs: [glyph(0, 10, 0), glyph(1, 20, 0, 20), glyph(2, 40, 0), glyph(3, 10, 20)] },
  { index: 1, glyphs: [glyph(1, 10, 40, 10, true), glyph(0, 20, 40, 10, true)] },
  { index: 2, glyphs: [], empty: { x: 10, y: 60, h: 18 } },
] });
const position = (block_index, offset, affinity = 'downstream') => ({ block_index, offset, affinity });
const collapsed = target => ({ anchor: target, focus: target });

test('pointer maps emoji cluster halves to whole grapheme offsets, including outside bounds', () => {
  assert.equal(hitTest(scene, 22, 8).offset, 1);
  assert.equal(hitTest(scene, 38, 8).offset, 2);
  assert.deepEqual(hitTest(scene, -100, -100), position(0, 0));
  assert.deepEqual(hitTest(scene, 10000, 10000), position(2, 0));
});
test('RTL pointer offsets follow visual edges rather than logical string order', () => {
  assert.equal(hitTest(scene, 11, 45).offset, 2);
  assert.equal(hitTest(scene, 19, 45).offset, 1);
  assert.equal(hitTest(scene, 29, 45).offset, 0);
});
test('reversed selection has identical geometry across wrapped lines and paragraphs', () => {
  const range = { anchor: position(0, 1), focus: position(1, 2) };
  assert.deepEqual(selectionRects(scene, range), selectionRects(scene, { anchor: range.focus, focus: range.anchor }));
  assert.deepEqual(selectionRects(scene, range).map(rect => rect.y), [0, 20, 40]);
});
test('arrows skip the emoji as one grapheme and preserve shift anchor', () => {
  const start = collapsed(position(0, 1));
  const next = navigate(scene, start, 'ArrowRight', true);
  assert.equal(next.focus.offset, 2);
  assert.deepEqual(next.anchor, start.anchor);
  assert.equal(caretRect(scene, next.focus).x, 40);
});
test('soft wrap navigation does not require an extra keypress at the same logical offset', () => {
  const next = navigate(scene, collapsed(position(0, 3, 'upstream')), 'ArrowRight');
  assert.equal(next.focus.offset, 4);
  assert.equal(caretRect(scene, next.focus).y, 20);
  assert.equal(navigate(scene, collapsed(position(0, 3)), 'ArrowLeft').focus.offset, 2);
});
test('visual RTL navigation and vertical desired column use Skia caret stops', () => {
  const next = navigate(scene, collapsed(position(1, 1)), 'ArrowLeft');
  assert.equal(next.focus.offset, 2);
  const down = navigate(scene, collapsed(position(0, 1)), 'ArrowDown', false, 20);
  assert.equal(down.focus.offset, 4);
});
test('home/end and document navigation include empty paragraphs', () => {
  assert.equal(navigate(scene, collapsed(position(0, 1)), 'End').focus.offset, 3);
  assert.deepEqual(navigate(scene, collapsed(position(0, 1)), 'DocumentEnd').focus, position(2, 0));
  assert.deepEqual(navigate(scene, collapsed(position(2, 0)), 'DocumentStart').focus, position(0, 0));
});
test('repeated navigation stays entirely local and stable under key repeat', () => {
  let selection = collapsed(position(0, 0));
  for (let i = 0; i < 10000; i++) selection = navigate(scene, selection, i % 2 ? 'ArrowLeft' : 'ArrowRight');
  assert.equal(selection.focus.offset, 0);
  assert.equal(selection.focus.block_index, 0);
});
