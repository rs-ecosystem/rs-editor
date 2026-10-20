// Skia geometry is immutable between document/viewport changes. All navigation
// and selection below is synchronous and independent of transport or the DOM.
export function prepareScene(scene) {
  const lines = [];
  for (const block of scene.blocks) {
    const blockLines = [];
    for (const glyph of block.glyphs) {
      let line = blockLines.find(line => Math.abs(line.y - glyph.y) < 0.5);
      if (!line) {
        line = { block: block.index, y: glyph.y, h: glyph.h, glyphs: [], stops: [] };
        blockLines.push(line);
      }
      line.h = Math.max(line.h, glyph.h);
      line.glyphs.push(glyph);
      line.stops.push(
        { x: glyph.x, offset: glyph.left, affinity: glyph.left_affinity },
        { x: glyph.x + glyph.w, offset: glyph.right, affinity: glyph.right_affinity },
      );
    }
    if (!blockLines.length && block.empty) {
      blockLines.push({ block: block.index, y: block.empty.y, h: block.empty.h, glyphs: [],
        stops: [{ x: block.empty.x, offset: 0, affinity: 'downstream' }] });
    }
    for (const line of blockLines) {
      line.glyphs.sort((a, b) => a.x - b.x);
      line.stops.sort((a, b) => a.x - b.x);
      const seen = new Set();
      line.stops = line.stops.filter(stop => {
        const key = `${Math.round(stop.x * 100)}:${stop.offset}:${stop.affinity}`;
        if (seen.has(key)) return false;
        seen.add(key); return true;
      });
      lines.push(line);
    }
  }
  lines.sort((a, b) => a.y - b.y);
  return { ...scene, lines };
}

const position = (line, stop) => ({ block_index: line.block, offset: stop.offset, affinity: stop.affinity || 'downstream' });
const distanceToBand = (value, start, size) => Math.max(start - value, value - start - size, 0);
export function hitTest(scene, x, y) {
  let closest = null;
  let distance = Infinity;
  for (const line of scene.lines) {
    const d = distanceToBand(y, line.y, line.h);
    if (d < distance) { closest = line; distance = d; }
  }
  if (!closest) return null;
  if (!closest.glyphs.length) return position(closest, closest.stops[0]);
  let glyph = closest.glyphs[0];
  let dx = Infinity;
  for (const candidate of closest.glyphs) {
    const d = distanceToBand(x, candidate.x, candidate.w);
    if (d < dx) { glyph = candidate; dx = d; }
  }
  const left = x < glyph.x + glyph.w / 2;
  return position(closest, { offset: left ? glyph.left : glyph.right,
    affinity: left ? glyph.left_affinity : glyph.right_affinity });
}

export function caretRect(scene, target) {
  if (!target) return null;
  const candidates = [];
  scene.lines.forEach((line, lineIndex) => {
    if (line.block !== target.block_index) return;
    for (const stop of line.stops) {
      if (stop.offset === target.offset) candidates.push({ ...stop, y: line.y, h: line.h, lineIndex });
    }
  });
  const exact = candidates.filter(stop => stop.affinity === target.affinity);
  const matches = exact.length ? exact : candidates;
  return target.affinity === 'upstream' ? matches[0] : matches.at(-1);
}

export const comparePositions = (a, b) => a.block_index - b.block_index || a.offset - b.offset;
export function selectionRects(scene, selection) {
  const [start, end] = comparePositions(selection.anchor, selection.focus) <= 0
    ? [selection.anchor, selection.focus] : [selection.focus, selection.anchor];
  const rects = [];
  for (const line of scene.lines) {
    if (line.block < start.block_index || line.block > end.block_index) continue;
    const from = line.block === start.block_index ? start.offset : 0;
    const to = line.block === end.block_index ? end.offset : Infinity;
    for (const glyph of line.glyphs) {
      if (glyph.offset < from || glyph.offset >= to) continue;
      const previous = rects.at(-1);
      if (previous && Math.abs(previous.y - glyph.y) < 0.1 && Math.abs(previous.h - glyph.h) < 0.1 && Math.abs(previous.x + previous.w - glyph.x) < 0.2) {
        previous.w += glyph.w;
      } else rects.push({ x: glyph.x, y: glyph.y, w: glyph.w, h: glyph.h });
    }
  }
  return rects;
}

export function navigate(scene, selection, key, extend = false, preferredX = null) {
  if (!scene.lines.length) return selection;
  let focus = selection.focus;
  if (!extend && comparePositions(selection.anchor, focus) !== 0 && (key === 'ArrowLeft' || key === 'ArrowRight')) {
    const endpoints = [selection.anchor, focus].sort(comparePositions);
    focus = key === 'ArrowLeft' ? endpoints[0] : endpoints[1];
    return { anchor: focus, focus };
  }
  const caret = caretRect(scene, focus);
  if (!caret) return selection;
  const line = scene.lines[caret.lineIndex];
  let targetLine = line;
  let stop;
  switch (key) {
    case 'ArrowLeft':
      stop = line.stops.filter(stop => stop.x < caret.x - 0.1).at(-1);
      if (!stop && caret.lineIndex > 0) {
        targetLine = scene.lines[caret.lineIndex - 1];
        stop = targetLine.stops.at(-1);
        if (targetLine.block === line.block && stop.offset === focus.offset) stop = targetLine.stops.filter(candidate => candidate.x < stop.x - 0.1).at(-1) || stop;
      }
      break;
    case 'ArrowRight':
      stop = line.stops.find(stop => stop.x > caret.x + 0.1);
      if (!stop && caret.lineIndex + 1 < scene.lines.length) {
        targetLine = scene.lines[caret.lineIndex + 1];
        stop = targetLine.stops[0];
        if (targetLine.block === line.block && stop.offset === focus.offset) stop = targetLine.stops.find(candidate => candidate.x > stop.x + 0.1) || stop;
      }
      break;
    case 'ArrowUp':
    case 'ArrowDown': {
      const index = caret.lineIndex + (key === 'ArrowUp' ? -1 : 1);
      if (scene.lines[index]) {
        targetLine = scene.lines[index];
        const x = preferredX ?? caret.x;
        stop = targetLine.stops.reduce((best, candidate) => Math.abs(candidate.x - x) < Math.abs(best.x - x) ? candidate : best);
      }
      break;
    }
    case 'Home': stop = line.stops[0]; break;
    case 'End': stop = line.stops.at(-1); break;
    case 'DocumentStart': targetLine = scene.lines[0]; stop = targetLine.stops[0]; break;
    case 'DocumentEnd': targetLine = scene.lines.at(-1); stop = targetLine.stops.at(-1); break;
  }
  if (stop) focus = position(targetLine, stop);
  return { anchor: extend ? selection.anchor : focus, focus };
}
