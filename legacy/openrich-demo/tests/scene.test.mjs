import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareScene, hitTest } from '../web/interaction.mjs';
const url = process.env.OPENRICH_DEMO_URL;

test('scene pixels/geometry are atomic and cached hits match native Skia at every DPI', { skip: !url }, async () => {
  const initial = await (await fetch(`${url}/api/state`)).json();
  try {
    for (const scale of [1, 2, 3]) {
      const response = await fetch(`${url}/api/scene?width=960&scale=${scale}`);
      assert.equal(response.status, 200);
      const data = await response.arrayBuffer();
      const length = new DataView(data).getUint32(0, true);
      const metadata = JSON.parse(new TextDecoder().decode(new Uint8Array(data, 4, length)));
      assert.deepEqual(metadata.document, initial.document);
      const pixels = new Uint8Array(data, 4 + length);
      assert.equal(metadata.format, 'rgba8');
      assert.equal(pixels.byteLength, metadata.pixel_width * metadata.pixel_height * 4);
      assert.deepEqual([...pixels.slice(0, 4)], [255, 255, 255, 255]);
      assert.equal(metadata.pixel_width, metadata.width * scale);
      assert.equal(metadata.pixel_height, metadata.height * scale);
      for (const value of Object.values(metadata.metrics)) assert.ok(Number.isFinite(value) && value >= 0);
      const scene = prepareScene(metadata);
      const points = [[-10, -10], [10000, 10000]];
      for (const block of metadata.blocks) {
        for (const glyph of block.glyphs.filter((_, i) => i % 3 === 0)) {
          for (const part of [0.25, 0.75]) points.push([glyph.x + glyph.w * part, glyph.y + glyph.h / 2]);
        }
      }
      for (const [x, y] of points) {
        const expected = await (await fetch(`${url}/api/pointer`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ x, y, width: 960, scale }) })).json();
        const actual = hitTest(scene, x, y);
        assert.equal(actual.block_index, expected.selection.focus.block_index, `block at ${x},${y},${scale}`);
        assert.equal(actual.offset, expected.selection.focus.offset, `offset at ${x},${y},${scale}`);
      }
    }
  } finally {
    await fetch(`${url}/api/action`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: 'set_selection', selection: initial.selection }) });
  }
});
