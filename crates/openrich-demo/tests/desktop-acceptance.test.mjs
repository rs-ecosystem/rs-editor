import test from 'node:test';
import assert from 'node:assert/strict';

// Runs the G3 acceptance checklist against desktop Chrome through the DevTools
// protocol. IME events come from Input.imeSetComposition/insertText, which drive
// Blink's composition pipeline but not a real OS input method or its candidate
// window. Screen reader output is not covered; the accessibility tree is.
const endpoint = process.env.OPENRICH_DESKTOP_CDP;
const base = process.env.OPENRICH_DESKTOP_URL?.replace(/\/?$/, '/');
const skip = !endpoint || !base;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const at = (block_index, offset) => ({block_index, offset, affinity:'downstream'});
const text = (value, marks = []) => ({type:'text', text:value, marks});
const paragraph = (id, value, marks) => ({type:'paragraph', id, content:value ? [text(value, marks)] : []});
const item = (id, value, list, checked = false) => ({type:'formatted', id, content:value ? [text(value)] : [], format:{alignment:'left', list, checked}});

async function state() { return (await fetch(`${base}api/state`)).json(); }
async function load(document) {
  const response = await fetch(`${base}api/load`, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(document)});
  assert.equal(response.status, 200);
}
function blockText(document) {
  return document.blocks.map(block => block.content ? block.content.map(run => run.text).join('') : `<${block.type}>`);
}

async function openPage() {
  const target = await (await fetch(`${endpoint}/json/new?about:blank`, {method:'PUT'})).json();
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let sequence = 0;
  const pending = new Map();
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    clearTimeout(entry.timer);
    message.error ? entry.reject(new Error(JSON.stringify(message.error))) : entry.resolve(message.result);
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 15000);
    pending.set(id, {resolve, reject, timer});
    socket.send(JSON.stringify({id, method, params}));
  });
  const evaluate = async expression => {
    const result = await send('Runtime.evaluate', {expression, awaitPromise:true, returnByValue:true});
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const ready = async () => {
    for (let attempt = 0; attempt < 200; attempt++) {
      if (await evaluate('document.querySelector("#loading")?.classList.contains("hidden") && !!document.querySelector("#editor [data-block]")').catch(() => false)) {
        await evaluate('document.fonts.ready.then(() => true)');
        return;
      }
      await sleep(50);
    }
    throw new Error('editor did not load');
  };
  await send('Page.enable');
  await send('Runtime.enable');
  // "Simpan JSON" must not write into the user's download folder.
  await send('Page.setDownloadBehavior', {behavior:'deny'});
  await send('Page.navigate', {url:base});
  await ready();
  const page = {
    send, evaluate,
    async reload() { await send('Page.reload', {ignoreCache:true}); await ready(); },
    // Places the native caret/range, as a pointer or arrow key would, and lets
    // the adapter observe it before the next input event.
    async select(anchor, focus = anchor) {
      await evaluate(`(async () => {
        const editor = document.querySelector('#editor');
        editor.focus({preventScroll:true});
        const point = ({block_index, offset}) => {
          const block = editor.querySelector('[data-block="' + block_index + '"]');
          const node = [...block.querySelectorAll('*'), block].flatMap(element => [...element.childNodes]).find(child => child.nodeType === 3);
          return [node, offset];
        };
        getSelection().setBaseAndExtent(...point(${JSON.stringify(anchor)}), ...point(${JSON.stringify(focus)}));
        document.dispatchEvent(new Event('selectionchange'));
        await new Promise(resolve => setTimeout(resolve, 120));
      })()`);
    },
    async preedit(steps) {
      for (const step of steps) {
        await send('Input.imeSetComposition', {text:step, selectionStart:step.length, selectionEnd:step.length});
        await sleep(30);
      }
    },
    // A null commit cancels the composition, as Escape does in an IME.
    async commit(value) {
      if (value == null) await send('Input.imeSetComposition', {text:'', selectionStart:0, selectionEnd:0});
      else await send('Input.insertText', {text:value});
      await page.settle();
    },
    async domTexts(count) {
      return evaluate(`Array.from({length:${count}}, (_, index) => document.querySelector('#editor [data-block="' + index + '"]')?.textContent ?? null)`);
    },
    async key(key, code, windowsVirtualKeyCode, command) {
      await send('Input.dispatchKeyEvent', {type:'rawKeyDown', key, code, windowsVirtualKeyCode, commands:command ? [command] : []});
      await send('Input.dispatchKeyEvent', {type:'keyUp', key, code, windowsVirtualKeyCode});
      await page.settle();
    },
    async click(selector) { await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`); await page.settle(); },
    // Waits until queued engine actions have returned.
    async settle() { await sleep(60); await evaluate('new Promise(resolve => setTimeout(resolve, 250))'); },
    async close() { socket.close(); await fetch(`${endpoint}/json/close/${target.id}`); },
  };
  return page;
}

async function withPage(document, run) {
  const original = await state();
  await load(document);
  const page = await openPage();
  try { await run(page); }
  finally {
    await page.close();
    await load(original.document);
  }
}

const richDocument = {schema_version:2, blocks:[
  {type:'heading', id:'91001', level:1, content:[text('Judul utama')]},
  {type:'paragraph', id:'91002', content:[text('Teks '), text('tebal', [{type:'bold'}]), text(' dan '), text('tautan', [{type:'link', href:'https://example.com'}])]},
  item('91003', 'Item satu', 'ordered'),
  item('91004', 'Item dua', 'ordered'),
  item('91005', 'Tugas selesai', 'checkbox', true),
  item('91006', 'Tugas belum', 'checkbox'),
  {type:'bullet_list', id:'91008', items:[{id:'91009', blocks:[paragraph('91010', 'Nested item')]}]},
  {type:'blockquote', id:'91011', blocks:[paragraph('91012', 'Kutipan')]},
  {type:'horizontal_rule', id:'91013'},
]};

test('accessibility tree exposes editor, headings, lists, checklist state and links', {skip}, async () => {
  await withPage(richDocument, async page => {
    await page.send('Accessibility.enable');
    const {nodes} = await page.send('Accessibility.getFullAXTree');
    const property = (node, name) => node.properties?.find(entry => entry.name === name)?.value.value;
    const find = (role, name) => nodes.filter(node => !node.ignored && node.role?.value === role && (name == null || node.name?.value === name));
    const [editor] = find('textbox', 'Dokumen OpenRich');
    assert.ok(editor, 'editor is a named textbox');
    assert.equal(property(editor, 'multiline'), true);
    assert.equal(property(editor, 'editable'), 'richtext');
    assert.equal(property(find('heading', 'Judul utama')[0], 'level'), 1);
    assert.equal(find('link', 'tautan').length, 1);
    assert.deepEqual(find('ListMarker').map(node => node.name.value), ['1. ', '2. ', '• ']);
    assert.equal(find('listitem').length, 5, 'two ordered, two checklist and one nested item');
    assert.equal(property(find('checkbox', 'Tugas selesai')[0], 'checked'), 'true');
    assert.equal(property(find('checkbox', 'Tugas belum')[0], 'checked'), 'false');
    assert.equal(find('blockquote').length, 1);
    assert.equal(find('separator').length, 1);
    const live = find('status');
    assert.equal(live.length, 1, 'one polite status region');
    assert.equal(property(live[0], 'live'), 'polite');
  });
});

test('routine typing leaves the live region quiet; file and error messages are announced', {skip}, async () => {
  await withPage({schema_version:2, blocks:[paragraph('92001', 'Halo')]}, async page => {
    await page.evaluate(`window.liveChanges = 0; new MutationObserver(records => window.liveChanges += records.length)
      .observe(document.querySelector('#announcer'), {childList:true, characterData:true, subtree:true}); true`);
    await page.select(at(0, 4));
    for (const character of [' ', 'd', 'u', 'n', 'i', 'a']) await page.send('Input.insertText', {text:character});
    await page.preedit(['に', '日本']);
    await page.commit('日本');
    assert.deepEqual(blockText((await state()).document), ['Halo dunia日本']);
    assert.equal(await page.evaluate('window.liveChanges'), 0);
    await page.evaluate(`document.querySelector('#announcer').textContent = ''; true`);
    await page.click('#saveButton');
    assert.equal(await page.evaluate(`document.querySelector('#announcer').textContent`), 'Dokumen JSON diunduh');
  });
});

test('IME started while typed text is still in flight lands after that text', {skip}, async () => {
  await withPage({schema_version:2, blocks:[paragraph('92101', 'Halo')]}, async page => {
    // Emulates a phone on LAN: engine replies arrive after the next IME event.
    await page.evaluate(`(() => { const original = window.fetch;
      window.fetch = async (...args) => { if (String(args[0]).includes('/api/action')) await new Promise(resolve => setTimeout(resolve, 150)); return original(...args); };
      return true; })()`);
    await page.select(at(0, 4));
    await page.send('Input.insertText', {text:' '});
    await page.send('Input.insertText', {text:'d'});
    await page.preedit(['あ', 'あい']);
    await page.send('Input.insertText', {text:'愛'});
    await page.send('Input.insertText', {text:'!'});
    await sleep(1200);
    const current = await state();
    assert.deepEqual(blockText(current.document), ['Halo d愛!']);
    assert.deepEqual(current.selection.focus, at(0, 8));
    assert.deepEqual(await page.domTexts(1), ['Halo d愛!']);
    await page.click('#undoButton');
    await sleep(600);
    assert.deepEqual(blockText((await state()).document), ['Halo d愛'], 'the composition and the later keystroke stay separate undo steps');
  });
});

test('Japanese, Chinese and Korean IME commit once, keep marks and undo as one step', {skip}, async () => {
  const cases = [
    {name:'ja after text', document:[paragraph('93001', 'Halo ')], caret:[at(0, 5)], steps:['k', 'か', 'かん', 'かんじ', '漢字'], commit:'漢字', after:['Halo 漢字']},
    {name:'ja replaces selection', document:[paragraph('93002', 'Halo dunia')], caret:[at(0, 5), at(0, 10)], steps:['せ', 'せかい', '世界'], commit:'世界', after:['Halo 世界'], undo:['Halo dunia']},
    {name:'ja cancelled', document:[paragraph('93003', 'Halo ')], caret:[at(0, 5)], steps:['に', 'にほん'], commit:null, after:['Halo ']},
    {name:'zh keeps bold', document:[paragraph('93004', 'Tebal', [{type:'bold'}])], caret:[at(0, 5)], steps:['n', 'ni', 'nihao', '你好'], commit:'你好', after:['Tebal你好'], marks:[[{type:'bold'}]]},
    {name:'ja in empty paragraph', document:[paragraph('93005', 'Atas'), paragraph('93006', '')], caret:[at(1, 0)], steps:['て', 'てす', 'テスト'], commit:'テスト', after:['Atas', 'テスト']},
    {name:'ko in heading', document:[{type:'heading', id:'93007', level:1, content:[text('Judul ')]}], caret:[at(0, 6)], steps:['ㅎ', '하', '한', '한ㄱ', '한그', '한글'], commit:'한글', after:['Judul 한글']},
    {name:'ja across paragraphs', document:[paragraph('93008', 'Satu dua'), paragraph('93009', 'Tiga empat')], caret:[at(0, 5), at(1, 4)], steps:['あ'], commit:'あ', after:['Satu あ empat'], undo:['Satu dua', 'Tiga empat']},
    {name:'zh beside nested list', document:[paragraph('93010', 'Halo '), {type:'bullet_list', id:'93011', items:[{id:'93012', blocks:[paragraph('93013', 'Anak')]}]}], caret:[at(0, 5)], steps:['r', 'ri', 'riben', '日本'], commit:'日本', after:['Halo 日本', '<bullet_list>']},
    {name:'ja in checklist item', document:[item('93014', 'Tugas', 'checkbox'), item('93015', 'Lain', 'checkbox')], caret:[at(0, 5)], steps:['て', 'テスト'], commit:'テスト', after:['Tugasテスト', 'Lain']},
  ];
  for (const scenario of cases) {
    const document = {schema_version:2, blocks:scenario.document};
    await withPage(document, async page => {
      await page.select(...scenario.caret);
      await page.preedit(scenario.steps);
      assert.deepEqual(blockText((await state()).document), blockText(document), `${scenario.name}: preedit is not committed`);
      await page.commit(scenario.commit);
      const after = await state();
      assert.deepEqual(blockText(after.document), scenario.after, scenario.name);
      const dom = await page.domTexts(after.document.blocks.length);
      assert.deepEqual(dom.filter((_, index) => after.document.blocks[index].content), scenario.after.filter(value => !value.startsWith('<')), `${scenario.name}: DOM matches engine`);
      if (scenario.marks) assert.deepEqual(after.document.blocks.map(block => block.content.at(-1).marks), scenario.marks, `${scenario.name}: marks`);
      await page.click('#undoButton');
      assert.deepEqual(blockText((await state()).document), scenario.undo || blockText(document), `${scenario.name}: one undo step`);
    });
  }
});

test('keyboard, checklist, paste and formatting acceptance flow', {skip}, async () => {
  await withPage({schema_version:2, blocks:[paragraph('94001', 'Halo'), item('94002', 'Tugas', 'checkbox'), paragraph('94003', 'Akhir')]}, async page => {
    await page.select(at(0, 4));
    await page.send('Input.insertText', {text:' 👩‍💻é'});
    await page.settle();
    await page.key('Backspace', 'Backspace', 8, 'deleteBackward');
    await page.key('Backspace', 'Backspace', 8, 'deleteBackward');
    assert.equal(blockText((await state()).document)[0], 'Halo ', 'backspace removes whole grapheme clusters');

    await page.select(at(1, 5));
    await page.key('Enter', 'Enter', 13, 'insertNewline');
    let current = await state();
    assert.deepEqual(current.document.blocks.slice(1, 3).map(block => block.format.list), ['checkbox', 'checkbox'], 'Enter continues the checklist');
    assert.deepEqual(current.selection.focus, at(2, 0));
    await page.key('Enter', 'Enter', 13, 'insertNewline');
    current = await state();
    assert.equal(current.document.blocks[2].format.list, 'none', 'Enter on an empty item exits the list');
    assert.deepEqual(blockText(current.document), ['Halo ', 'Tugas', '', 'Akhir']);
    await page.send('Input.insertText', {text:'x'});
    await page.settle();
    assert.deepEqual(blockText((await state()).document), ['Halo ', 'Tugas', 'x', 'Akhir'], 'text lands in the block that holds the caret');
    await page.key('Backspace', 'Backspace', 8, 'deleteBackward');
    await page.key('Backspace', 'Backspace', 8, 'deleteBackward');
    assert.deepEqual(blockText((await state()).document), ['Halo ', 'Tugas', 'Akhir'], 'backspace at block start joins');

    await page.evaluate(`document.querySelector('#editor [data-block="1"]').parentElement.querySelector('.semantic-check').click(); true`);
    await page.settle();
    assert.equal((await state()).document.blocks[1].format.checked, true, 'assistive technology can toggle the checklist item');
    await page.click('#undoButton');
    assert.equal((await state()).document.blocks[1].format.checked, false);

    await page.select(at(2, 5));
    await page.evaluate(`(() => { const data = new DataTransfer(); data.setData('text/plain', ' tempel');
      document.querySelector('#editor').dispatchEvent(new ClipboardEvent('paste', {clipboardData:data, bubbles:true, cancelable:true})); return true; })()`);
    await page.settle();
    assert.equal(blockText((await state()).document)[2], 'Akhir tempel');

    await page.select(at(0, 0), at(0, 4));
    await page.click('[data-mark="bold"]');
    assert.deepEqual((await state()).document.blocks[0].content[0], text('Halo', [{type:'bold'}]));
    assert.equal(await page.evaluate(`document.querySelector('[data-mark="bold"]').getAttribute('aria-pressed')`), 'true');
    await page.click('#undoButton');
    assert.deepEqual((await state()).document.blocks[0].content[0].marks, []);
    await page.click('#redoButton');
    assert.deepEqual((await state()).document.blocks[0].content[0].marks, [{type:'bold'}]);
  });
});

test('saved JSON reloads identically and the Skia frame follows resize and DPI', {skip}, async () => {
  await withPage(richDocument, async page => {
    const saved = (await state()).document;
    await load(JSON.parse(JSON.stringify(saved)));
    await page.reload();
    assert.deepEqual((await state()).document, saved);
    const domText = await page.evaluate(`document.querySelector('#editor').textContent`);
    assert.equal(domText, blockText(saved).filter(value => !value.startsWith('<')).join('') + 'Nested itemKutipan');
    for (const [width, deviceScaleFactor] of [[1280, 1], [700, 2], [420, 3]]) {
      await page.send('Emulation.setDeviceMetricsOverride', {width, height:900, deviceScaleFactor, mobile:false});
      let size;
      for (let attempt = 0; attempt < 60; attempt++) {
        await sleep(100);
        size = await page.evaluate(`(() => { const surface = document.querySelector('#surface'), canvas = document.querySelector('#skiaCanvas');
          return {expected:Math.max(320, Math.round(surface.clientWidth)) * devicePixelRatio, actual:canvas.width, scale:canvas.dataset.scale}; })()`);
        if (size.actual === size.expected) break;
      }
      assert.equal(size.actual, size.expected, `canvas width at ${width}px @${deviceScaleFactor}x`);
      assert.equal(size.scale, String(deviceScaleFactor));
    }
    await page.send('Emulation.clearDeviceMetricsOverride');
  });
});

test('native caret matches Skia at every grapheme in Arabic, Devanagari and Hebrew', {skip}, async () => {
  // Android positions its native caret handle from DOM text; any font that
  // differs from Skia's moves that handle away from the painted caret.
  const document = {schema_version:2, blocks:[
    paragraph('93001', 'Unicode: emoji 🙂 · עברית · العربية · हिन्दी'),
    paragraph('93002', 'العربية हिन्दी עברית', [{type:'bold'}]),
  ]};
  await withPage(document, async page => {
    const endpoints = await page.evaluate(`(async () => {
      const editor = document.querySelector('#editor'), canvas = document.querySelector('#skiaCanvas').getBoundingClientRect();
      const response = await fetch('/api/scene?width=' + document.querySelector('#surface').clientWidth + '&scale=' + devicePixelRatio + '&format=png');
      const bytes = await response.arrayBuffer(), length = new DataView(bytes).getUint32(0, true);
      const {prepareScene, caretRect} = await import('/interaction.mjs');
      const scene = prepareScene(JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 4, length))));
      const segmenter = new Intl.Segmenter(undefined, {granularity:'grapheme'}), results = [];
      for (const block of editor.querySelectorAll('[data-block]')) {
        const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
        let node, offset = 0;
        while ((node = walker.nextNode())) for (const {index, segment} of segmenter.segment(node.textContent)) {
          offset += 1;
          const range = document.createRange();
          range.setStart(node, index + segment.length); range.collapse(true);
          const rect = range.getClientRects()[0];
          const caret = caretRect(scene, {block_index:Number(block.dataset.block), offset, affinity:'upstream'});
          if (rect && caret) results.push({block:block.dataset.block, offset, segment, dx:rect.left - canvas.left - caret.x});
        }
      }
      return results;
    })()`);
    assert.equal(endpoints.length, 39 + 16, 'every grapheme boundary must be measured');
    for (const endpoint of endpoints) assert.ok(Math.abs(endpoint.dx) < 0.5, `horizontal mismatch ${JSON.stringify(endpoint)}`);
  });
});
