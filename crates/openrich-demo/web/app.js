import { prepareScene, hitTest, caretRect, selectionRects, navigate, comparePositions } from '/interaction.mjs';
import { replacementDiff, typingMarks, typingStyle, sceneMatches, wordRange, touchMovement, isDoubleTap, selectionOwner, preeditDocument, documentFingerprint } from '/input.mjs';
const editor = document.querySelector('#editor');
const surface = document.querySelector('#surface');
const canvas = document.querySelector('#skiaCanvas');
const status = document.querySelector('#status');
const loading = document.querySelector('#loading');
const preview = document.querySelector('#compositionPreview');
const fileInput = document.querySelector('#fileInput');
const overlay = document.querySelector('#selectionCanvas');
const overlayContext = overlay.getContext('2d');
let currentState = null;
let composing = false;
let compositionSelection = null;
let compositionSnapshot = null;
let compositionDocument = null;
let previewDocument = null;
let compositionEpoch = 0;
let previewRenderQueued = false;
let nativeSelectionActive = false;
let selectionRetryTimer = 0;
let compositionMarks = [];
let compositionStyleFrame = 0;
let compositionDirty = false;
let compositionWaiters = [];
let nativeSelectionGuard = 0;
let paintedSelection = null;
let compositionTail = null;
let touchMode = false;
let handleDrag = null;
const selectionHandles = [document.querySelector('#selectionStart'), document.querySelector('#selectionEnd')];
let historyTimer = 0;
let insertTimer = 0;
let pendingInsert = '';
let pendingInsertSelection = null;
let pendingCaret = null;
let actionQueue = Promise.resolve();
let actionGeneration = 0;
let canvasResizeObserver = null;
let pointerSession = null;
let layoutSnapshot = null;
let documentKey = '';
let selectionEpoch = 0;
let preferredCaretX = null;
let overlayFrame = 0;
let overlayInputTime = 0;
let sceneRequest = 0;
let sceneController = null;
let scenePending = false;
let activeSceneKey = null;
let sceneFrames = [];
let sceneMetrics = null;
let selectionTimings = [];
let selectionCount = 0;
let sceneSchedule = 0;
let sceneRequestCount = 0;
let actionRequestCount = 0;
let mutationPending = 0;
let deferredInputs = [];
const segmenter = typeof Intl.Segmenter === 'function'
  ? new Intl.Segmenter(undefined, { granularity: 'grapheme' })
  : null;

function graphemes(text) {
  return segmenter ? [...segmenter.segment(text)].map(item => item.segment) : Array.from(text);
}
function graphemeOffset(text) { return graphemes(text).length; }
function utf16Offset(text, offset) {
  return graphemes(text).slice(0, offset).join('').length;
}

function renderInline(content, host) {
  for (const item of content || []) {
    if (item.type !== 'text') continue;
    const marks = item.marks || [];
    let node = document.createElement('span');
    node.className = 'text-run';
    node.textContent = item.text || '';
    if (!node.textContent) node.append(document.createTextNode(''));
    for (const mark of marks) {
      let wrapper;
      if (mark.type === 'bold') wrapper = document.createElement('strong');
      else if (mark.type === 'italic') wrapper = document.createElement('em');
      else if (mark.type === 'underline') wrapper = document.createElement('u');
      else if (mark.type === 'strike') wrapper = document.createElement('s');
      else if (mark.type === 'code') wrapper = document.createElement('code');
      else if (mark.type === 'link') { wrapper = document.createElement('a'); wrapper.href = mark.href; }
      if (wrapper) { wrapper.append(node); node = wrapper; }
    }
    host.append(node);
  }
  if (!host.hasChildNodes()) host.append(document.createTextNode(''));
}

function makeBlock(block, index) {
  let element;
  switch (block.type) {
    case 'formatted': element = document.createElement(block.format?.heading_level ? `h${block.format.heading_level}` : 'p'); break;
    case 'heading': element = document.createElement(`h${Math.min(6, Math.max(1, block.level || 1))}`); break;
    case 'bullet_list': element = document.createElement('ul'); break;
    case 'ordered_list': element = document.createElement('ol'); element.start = block.start || 1; break;
    case 'blockquote': element = document.createElement('blockquote'); break;
    case 'horizontal_rule': element = document.createElement('hr'); break;
    default: element = document.createElement('p');
  }
  if (block.format) {
    element.style.textAlign = block.format.alignment || 'left';
    if (block.format.list !== 'none') element.style.paddingLeft = '32px';
  }
  element.dataset.block = String(index);
  if (block.id) element.dataset.nodeId = block.id;
  if (block.type === 'paragraph' || block.type === 'heading' || block.type === 'formatted') renderInline(block.content, element);
  else if (block.type === 'bullet_list' || block.type === 'ordered_list') {
    for (const item of block.items || []) {
      const li = document.createElement('li');
      for (const child of item.blocks || []) li.append(makeBlock(child, index));
      element.append(li);
    }
  } else if (block.type === 'blockquote') {
    for (const child of block.blocks || []) element.append(makeBlock(child, index));
  }
  return element;
}

function renderSemanticDocument() {
  if (!currentState) return;
  editor.replaceChildren(...currentState.document.blocks.map(makeBlock));
  restoreSelection(currentState.selection);
}

function restoreEndpoint(position, edge) {
  const block = editor.querySelector(`[data-block="${position.block_index}"]`);
  if (!block) return null;
  const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
  let node;
  let remaining = position.offset;
  let last = null;
  while ((node = walker.nextNode())) {
    last = node;
    const count = graphemeOffset(node.nodeValue || '');
    if (remaining <= count) return [node, utf16Offset(node.nodeValue || '', remaining)];
    remaining -= count;
  }
  if (!last) {
    const empty = document.createTextNode('');
    block.append(empty);
    return [empty, 0];
  }
  return [last, (last.nodeValue || '').length];
}

function restoreSelection(selection) {
  if (!selection || !editor.isConnected) return;
  const anchor = restoreEndpoint(selection.anchor, 'anchor');
  const focus = restoreEndpoint(selection.focus, 'focus');
  if (!anchor || !focus) return;
  const native = window.getSelection();
  try {
    if (native.anchorNode === anchor[0] && native.anchorOffset === anchor[1] && native.focusNode === focus[0] && native.focusOffset === focus[1]) return;
    nativeSelectionGuard = performance.now() + 80;
    native.setBaseAndExtent(anchor[0], anchor[1], focus[0], focus[1]);
  } catch (_) { /* The block can be structural and non-editable. */ }
}

function endpointFromDom(node, offset) {
  const element = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
  const block = element?.closest('[data-block]');
  if (!block) return null;
  const range = document.createRange();
  range.selectNodeContents(block);
  try { range.setEnd(node, offset); } catch (_) { return null; }
  const position = { block_index: Number(block.dataset.block), offset: graphemeOffset(range.toString()), affinity: 'downstream' };
  return position;
}

function readNativeSelection() {
  const native = window.getSelection();
  if (!native?.anchorNode || !editor.contains(native.anchorNode)) return null;
  const anchor = endpointFromDom(native.anchorNode, native.anchorOffset);
  const focus = endpointFromDom(native.focusNode, native.focusOffset);
  return anchor && focus ? {anchor, focus} : null;
}

function readSelection() {
  if (pendingCaret) return pendingCaret;
  if (pointerSession) return currentState?.selection || null;
  const native = window.getSelection();
  if (!native || !native.anchorNode || !editor.contains(native.anchorNode)) return currentState?.selection || null;
  const anchor = endpointFromDom(native.anchorNode, native.anchorOffset);
  const focus = endpointFromDom(native.focusNode, native.focusOffset);
  return anchor && focus ? { anchor, focus } : currentState?.selection || null;
}

async function updateState(state, rerender = true, preserveSelection = false) {
  const latestSelection = preserveSelection ? currentState?.selection : null;
  const nextKey = documentFingerprint(state.document);
  const changed = nextKey !== documentKey;
  if (changed && layoutSnapshot && !paintedSelection) paintedSelection = structuredClone(currentState.selection);
  currentState = state;
  if (latestSelection) currentState.selection = latestSelection;
  else pendingCaret = null;
  updateBlockToolbar();
  if (composing) scheduleCompositionStyle();
  documentKey = nextKey;
  if (changed || (rerender && !composing)) {
    if (rerender && !composing) renderSemanticDocument();
    if (changed && !sceneSchedule) sceneSchedule = requestAnimationFrame(() => { sceneSchedule = 0; renderCanvas(); });
  } else if (!composing) restoreSelection(currentState.selection);
  scheduleOverlay();
  status.textContent = composing ? 'Menulis dengan IME…' : 'Tersimpan di engine lokal';
}

// One scene request returns text pixels and matching geometry atomically.
// Moving a caret never enters this path. Older content/resize replies are dropped.
async function renderCanvas(force = false) {
  const width = Math.min(3200, Math.max(320, Math.round(surface.clientWidth || 960)));
  const scale = Math.min(3, Math.max(1, window.devicePixelRatio || 1));
  if (composing && scenePending) { previewRenderQueued = true; return; }
  const renderedDocument = composing && previewDocument ? previewDocument : currentState.document;
  const key = documentFingerprint(renderedDocument);
  const sceneKey = `${key}|${width}|${scale}`;
  if (!force && sceneKey === activeSceneKey && (layoutSnapshot || scenePending)) return;
  activeSceneKey = sceneKey;
  scenePending = true;
  canvas.dataset.scale = String(scale);
  // Keep the last matching pixels/geometry until the replacement scene arrives.
  scheduleOverlay();
  const token = ++sceneRequest;
  const isPreview = composing && !!previewDocument;
  const previewEpoch = compositionEpoch;
  const validFrame = () => isPreview ? composing && previewEpoch === compositionEpoch : matchesCurrent();
  const matchesCurrent = () => key === documentFingerprint(composing && previewDocument ? previewDocument : currentState.document);
  sceneController?.abort();
  sceneController = new AbortController();
  try {
    sceneRequestCount += 1;
    const response = await fetch(`/api/${isPreview ? "preview-scene" : "scene"}?width=${width}&scale=${scale}&format=${location.hostname === "127.0.0.1" || location.hostname === "localhost" ? "rgba" : "png"}`, { signal: sceneController.signal, ...(isPreview ? {method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(renderedDocument)} : {}) });
    if (!response.ok) throw new Error((await response.json()).error || `HTTP ${response.status}`);
    const buffer = await response.arrayBuffer();
    const metadataLength = new DataView(buffer).getUint32(0, true);
    const metadata = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 4, metadataLength)));
    if (token !== sceneRequest || !validFrame() || documentFingerprint(metadata.document) !== key) return;
    let image = null;
    if (metadata.format === 'png') {
      image = await createImageBitmap(new Blob([buffer.slice(4 + metadataLength)], { type: 'image/png' }));
      if (token !== sceneRequest || !validFrame()) { image.close(); return; }
    }
    canvas.width = metadata.pixel_width;
    canvas.height = metadata.pixel_height;
    const context = canvas.getContext('2d', { alpha: false });
    if (image) { context.drawImage(image, 0, 0); image.close(); }
    else {
      const pixels = new Uint8ClampedArray(buffer, 4 + metadataLength);
      context.putImageData(new ImageData(pixels, metadata.pixel_width, metadata.pixel_height), 0, 0);
    }
    layoutSnapshot = prepareScene(metadata);
    paintedSelection = null;
    flushDeferredInputs();
    sceneFrames.push(performance.now());
    sceneMetrics = metadata.metrics;
    loading.classList.add('hidden');
    scheduleOverlay();
  } catch (error) {
    if (error.name !== 'AbortError' && token === sceneRequest) {
      activeSceneKey = null;
      status.textContent = error.message;
      loading.textContent = `Gagal merender: ${error.message}`;
    }
  } finally {
    if (token === sceneRequest) {
      scenePending = false;
      if (previewRenderQueued) { previewRenderQueued = false; if (composing) renderCanvas(); }
    }
  }
}

function scheduleOverlay(inputTime = 0) {
  if (inputTime) overlayInputTime = inputTime;
  if (overlayFrame) return;
  overlayFrame = requestAnimationFrame(() => {
    overlayFrame = 0;
    paintOverlay();
    if (overlayInputTime) {
      selectionTimings.push(performance.now() - overlayInputTime);
      selectionCount += 1;
      if (selectionTimings.length > 200) selectionTimings.shift();
      performance.measure('openrich-selection-paint', { start: overlayInputTime, end: performance.now() });
      if (performance.getEntriesByName('openrich-selection-paint').length > 200) performance.clearMeasures('openrich-selection-paint');
      overlayInputTime = 0;
    }
  });
}

function paintOverlay() {
  const width = surface.clientWidth;
  const height = surface.clientHeight;
  const dpr = window.devicePixelRatio || 1;
  const pixelWidth = Math.round(width * dpr);
  const pixelHeight = Math.round(height * dpr);
  if (overlay.width !== pixelWidth || overlay.height !== pixelHeight) {
    overlay.width = pixelWidth; overlay.height = pixelHeight;
  }
  overlayContext.setTransform(1, 0, 0, 1, 0, 0);
  overlayContext.clearRect(0, 0, overlay.width, overlay.height);
  for (const handle of selectionHandles) handle.hidden = true;
  if (!layoutSnapshot || !currentState) return;
  // Skia owns text and highlight in both committed and preedit frames.
  surface.dataset.selectionOwner = selectionOwner(composing);

  const factor = width / layoutSnapshot.width * dpr;
  overlayContext.setTransform(factor, 0, 0, factor, 0, 0);
  const nativeSelection = readNativeSelection();
  const selection = composing || nativeSelectionActive ? (nativeSelection || currentState.selection) : (sceneMatches(layoutSnapshot, documentKey) ? currentState.selection : (paintedSelection || currentState.selection));
  if (!selection) return;
  if (comparePositions(selection.anchor, selection.focus) === 0) {
    const caret = caretRect(layoutSnapshot, selection.focus);
    if (caret && editor.contains(document.activeElement)) {
      overlayContext.fillStyle = '#1854b8';
      overlayContext.fillRect(caret.x, caret.y, 1.25, caret.h);
    }
  } else {
    overlayContext.fillStyle = 'rgba(53,122,216,0.32)';
    for (const rect of selectionRects(layoutSnapshot, selection)) overlayContext.fillRect(rect.x, rect.y, rect.w, rect.h);
    if (touchMode && !composing && !nativeSelectionActive) {
      const scale = width / layoutSnapshot.width;
      [selection.anchor, selection.focus].forEach((position, index) => {
        const rect = caretRect(layoutSnapshot, position);
        if (!rect) return;
        const handle = selectionHandles[index];
        handle.hidden = false;
        handle.style.left = `${rect.x * scale}px`;
        handle.style.top = `${(rect.y + rect.h) * scale}px`;
      });
    }
  }
}

function flushDeferredInputs() {
  if (composing || !sceneMatches(layoutSnapshot, documentKey) || mutationPending) return;
  const inputs = deferredInputs;
  deferredInputs = [];
  for (const input of inputs) input();
}

function navigateLocal(key, extend, started) {
  if (key === 'ArrowUp' || key === 'ArrowDown') preferredCaretX ??= caretRect(layoutSnapshot, currentState.selection.focus)?.x;
  else preferredCaretX = null;
  setLocalSelection(navigate(layoutSnapshot, currentState.selection, key, extend, preferredCaretX), started);
}

function setLocalSelection(selection, inputTime = performance.now()) {
  if (!selection || !currentState) return;
  selectionEpoch += 1;
  pendingCaret = null;
  if (comparePositions(currentState.selection.anchor, selection.anchor) !== 0 || comparePositions(currentState.selection.focus, selection.focus) !== 0) currentState.active_marks = null;
  currentState.selection = selection;
  updateBlockToolbar();
  restoreSelection(selection);
  scheduleOverlay(inputTime);
}

// UI cadence and committed Skia redraws are different measurements. Skia is
// event-driven: zero redraw FPS while idle is expected and saves work.
let hudStart = performance.now();
let hudFrames = 0;
function performanceHUD(now) {
  hudFrames += 1;
  if (now - hudStart >= 500) {
    document.querySelector('#uiFPS').textContent = Math.round(hudFrames * 1000 / (now - hudStart));
    sceneFrames = sceneFrames.filter(time => now - time < 1000);
    document.querySelector('#skiaFPS').textContent = sceneFrames.length;
    const sorted = [...selectionTimings].sort((a, b) => a - b);
    const p95 = sorted[Math.max(0, Math.ceil(sorted.length * 0.95) - 1)] || 0;
    document.querySelector('#inputMs').textContent = p95.toFixed(1);
    const hud = document.querySelector('.performance-hud');
    hud.dataset.sceneRequests = String(sceneRequestCount);
    hud.dataset.actionRequests = String(actionRequestCount);
    hud.dataset.selectionCount = String(selectionCount);
    hud.dataset.selectionP95 = String(p95);
    hud.dataset.selectionMax = String(sorted.at(-1) || 0);
    if (sceneMetrics) {
      document.querySelector('#skiaMs').textContent = sceneMetrics.raster_ms.toFixed(1);
      document.querySelector('#sceneMs').textContent = sceneMetrics.total_ms.toFixed(1);
      document.querySelector('#sceneMs').parentElement.title = `Layout ${sceneMetrics.layout_ms.toFixed(1)} ms · geometri ${sceneMetrics.geometry_ms.toFixed(1)} ms · raster ${sceneMetrics.raster_ms.toFixed(1)} ms · transfer ${sceneMetrics.transfer_ms.toFixed(1)} ms`;
    }
    hudStart = now; hudFrames = 0;
  }
  requestAnimationFrame(performanceHUD);
}
requestAnimationFrame(performanceHUD);

async function post(path, value, raw = false) {
  actionRequestCount += 1;
  const response = await fetch(path, {
    method: 'POST',
    headers: raw ? { 'Content-Type': 'application/json' } : { 'Content-Type': 'application/json' },
    body: raw ? value : JSON.stringify(value),
  });
  const result = await response.json();
  if (!response.ok) { const error = new Error(result.error || `HTTP ${response.status}`); error.status = response.status; throw error; }
  return result;
}

function action(command, options = {}) {
  const value = { command, ...options };
  // Capture at input time, not after older queued requests have changed the DOM.
  if (!options.noSelection && !options.selection) value.selection = readSelection();
  delete value.noSelection; delete value.keepDom; delete value.message; delete value.preserveSelection;
  const epoch = selectionEpoch;
  const generation = actionGeneration;
  const mutation = ['insert_text','delete_backward','delete_forward','split_block','set_block_format','toggle_mark','undo','redo','composition_commit','composition_replace'].includes(command);
  if (mutation) mutationPending += 1;
  actionQueue = actionQueue.then(async () => {
    try {
      if (generation !== actionGeneration) return;
      value.expected_document = currentState.document;
      const state = await post('/api/action', value);
      await updateState(state, options.keepDom !== true, options.preserveSelection === true || epoch !== selectionEpoch || options.keepDom === true);
      if (options.message) status.textContent = options.message;
    } catch (error) {
      if (error.status === 409) {
        actionGeneration += 1;
        deferredInputs = [];
        pendingInsert = ''; pendingInsertSelection = null; pendingCaret = null;
        composing = false; surface.classList.remove('composing');
        const response = await fetch('/api/state');
        await updateState(await response.json());
        const waiters = compositionWaiters; compositionWaiters = [];
        for (const resolve of waiters) resolve();
      }
      status.textContent = error.message;
    }
    finally { if (mutation) mutationPending -= 1; flushDeferredInputs(); }
  }).catch(error => { status.textContent = error.message; });
  if (command === 'insert_text' || command === 'composition_commit' || command === 'composition_replace') {
    clearTimeout(historyTimer);
    historyTimer = setTimeout(() => action('end_history_group', { noSelection: true, keepDom: true }), 750);
  }
  return actionQueue;
}

function flushPendingInsert() {
  clearTimeout(insertTimer);
  if (!pendingInsert) return;
  const text = pendingInsert;
  const selection = pendingInsertSelection;
  if (selection) {
    const anchor = selection.anchor;
    const focus = selection.focus;
    const start = anchor.block_index < focus.block_index || (anchor.block_index === focus.block_index && anchor.offset <= focus.offset) ? anchor : focus;
    pendingCaret = {
      anchor: { block_index: start.block_index, offset: start.offset + graphemeOffset(text), affinity: 'downstream' },
      focus: { block_index: start.block_index, offset: start.offset + graphemeOffset(text), affinity: 'downstream' },
    };
  }
  pendingInsert = '';
  pendingInsertSelection = null;
  selectionEpoch += 1;
  action('insert_text', { text, selection });
}

function selectionchange() {
  if (composing) { scheduleCompositionStyle(); scheduleOverlay(); return; }
  if (performance.now() < nativeSelectionGuard) {
    clearTimeout(selectionRetryTimer);
    selectionRetryTimer = setTimeout(selectionchange, Math.ceil(nativeSelectionGuard-performance.now()) + 1);
    return;
  }
  if (handleDrag) return;
  const selection = readNativeSelection();
  const nativeRange = selection && comparePositions(selection.anchor, selection.focus) !== 0;
  if (nativeRange && touchMode) { nativeSelectionActive = true; scheduleOverlay(); }
  if (composing || (pointerSession && !nativeRange)) return;
  if (selection && (comparePositions(selection.anchor, currentState.selection.anchor) !== 0 || comparePositions(selection.focus, currentState.selection.focus) !== 0)) {
    nativeSelectionActive = touchMode && comparePositions(selection.anchor, selection.focus) !== 0;
    setLocalSelection(selection);
  }
}

function queuePointer(session, event) {
  const started = performance.now();
  if (composing || !sceneMatches(layoutSnapshot, documentKey) || mutationPending) {
    const point = { clientX: event.clientX, clientY: event.clientY };
    if (!session.deferredInitial) session.deferredInitial = point;
    session.deferredLatest = point;
    if (!session.deferred) {
      session.deferred = true;
      deferredInputs.push(() => {
        session.deferred = false;
        queuePointer(session, session.deferredInitial);
        queuePointer(session, session.deferredLatest);
        session.deferredInitial = null;
      });
    }
    return;
  }
  const rect = canvas.getBoundingClientRect();
  const factor = layoutSnapshot.width / rect.width;
  const focus = hitTest(layoutSnapshot, (event.clientX - rect.left) * factor, (event.clientY - rect.top) * factor);
  if (!focus) return;
  if (!session.anchor) session.anchor = focus;
  preferredCaretX = null;
  setLocalSelection({ anchor: session.anchor, focus }, started);
}

let lastTouchTap = null;
let ignoreMouseClicksUntil = 0;
surface.addEventListener('pointerdown', event => {
  if (event.target.closest('.selection-handle')) return;
  if (event.button !== 0) return;
  nativeSelectionGuard = 0;
  // Let Android move the native caret/commit IME without dismissing keyboard.
  if (composing && event.pointerType === "touch") return;
  touchMode = event.pointerType === 'touch';
  nativeSelectionActive = false;
  if (!composing && sceneMatches(layoutSnapshot, documentKey) && !mutationPending) {
    const bounds = canvas.getBoundingClientRect();
    const scale = layoutSnapshot.width / bounds.width;
    const x = (event.clientX - bounds.left) * scale, y = (event.clientY - bounds.top) * scale;
    const item = layoutSnapshot.blocks.find(block => block.checkbox && x >= block.checkbox.x && x <= block.checkbox.x + block.checkbox.w && y >= block.checkbox.y && y <= block.checkbox.y + block.checkbox.h);
    if (item) {
      event.preventDefault(); flushPendingInsert();
      const format = currentState.document.blocks[item.index].format;
      const position = {block_index:item.index, offset:0, affinity:'downstream'};
      action('set_block_format', {format:{...format, checked:!format.checked}, selection:{anchor:position, focus:position}});
      return;
    }
  }
  if (event.pointerType === 'touch') {
    event.preventDefault();
    flushPendingInsert();
    pointerSession = { id: event.pointerId, touch: true, x: event.clientX, y: event.clientY, lastY: event.clientY, time: performance.now(), moved: false, selecting: false };
    const session = pointerSession;
    const point = { clientX: event.clientX, clientY: event.clientY };
    session.holdTimer = setTimeout(() => {
      if (pointerSession !== session || session.moved) return;
      const begin = () => {
        if (pointerSession !== session || session.moved) return;
        session.selecting = true;
        queuePointer(session, point);
        selectPointerUnit();
        session.anchor = currentState.selection.anchor;
        ignoreMouseClicksUntil = performance.now() + 700;
      };
      if (composing || mutationPending || !sceneMatches(layoutSnapshot, documentKey)) deferredInputs.push(begin); else begin();
    }, 450);
    surface.setPointerCapture(event.pointerId);
    return;
  }
  event.preventDefault();
  flushPendingInsert();
  editor.focus({ preventScroll: true });
  pointerSession = { id: event.pointerId, anchor: event.shiftKey ? readSelection()?.anchor : null };
  surface.setPointerCapture(event.pointerId);
  queuePointer(pointerSession, event);
});
surface.addEventListener('pointermove', event => {
  if (!pointerSession || event.pointerId !== pointerSession.id) return;
  if (pointerSession.touch) {
    event.preventDefault();
    const movement = touchMovement(pointerSession, event.clientX, event.clientY);
    if (movement === 'selection') { queuePointer(pointerSession, event); return; }
    if (movement === 'scroll') {
      pointerSession.moved = true;
      clearTimeout(pointerSession.holdTimer);
    }
    if (pointerSession.moved) window.scrollBy(0, pointerSession.lastY - event.clientY);
    pointerSession.lastY = event.clientY;
    return;
  }
  event.preventDefault();
  queuePointer(pointerSession, event);
});
surface.addEventListener('pointerup', event => {
  if (!pointerSession || event.pointerId !== pointerSession.id) return;
  if (pointerSession.touch) {
    const session = pointerSession;
    pointerSession = null;
    if (surface.hasPointerCapture(event.pointerId)) surface.releasePointerCapture(event.pointerId);
    clearTimeout(session.holdTimer);
    if (session.selecting || session.moved) { event.preventDefault(); lastTouchTap = null; return; }
    event.preventDefault();
    ignoreMouseClicksUntil = performance.now() + 700;
    flushPendingInsert();
    if (!composing) editor.focus({ preventScroll: true });
    const doubleTap = isDoubleTap(lastTouchTap, session);
    const applyTap = () => {
      queuePointer(session, event);
      if (doubleTap) selectPointerUnit();
    };
    if (composing || mutationPending || !sceneMatches(layoutSnapshot, documentKey)) deferredInputs.push(applyTap);
    else applyTap();
    lastTouchTap = doubleTap ? null : session;
    return;
  }
  event.preventDefault();
  queuePointer(pointerSession, event);
  surface.releasePointerCapture(event.pointerId);
  pointerSession = null;
});
surface.addEventListener('pointercancel', () => { clearTimeout(pointerSession?.holdTimer); pointerSession = null; lastTouchTap = null; });

function selectPointerUnit(wholeBlock = false) {
  if (composing || mutationPending || !sceneMatches(layoutSnapshot, documentKey)) { deferredInputs.push(() => selectPointerUnit(wholeBlock)); return; }
  const position = currentState?.selection?.focus;
  const block = currentState?.document.blocks[position?.block_index];
  if (!position || !block?.content) return;
  const text = block.content.filter(item => item.type === 'text').map(item => item.text).join('');
  let start = 0;
  let end = text.length;
  if (!wholeBlock) {
    const offset = utf16Offset(text, position.offset);
    const range = wordRange(text, offset, position.affinity);
    if (!range) return;
    start = range.start;
    end = range.end;
  }
  setLocalSelection({
    anchor: { ...position, offset: graphemeOffset(text.slice(0, start)) },
    focus: { ...position, offset: graphemeOffset(text.slice(0, end)) },
  });
}
surface.addEventListener('dblclick', event => {
  event.preventDefault();
  if (performance.now() >= ignoreMouseClicksUntil) selectPointerUnit();
});
surface.addEventListener('click', event => {
  if (performance.now() < ignoreMouseClicksUntil) { event.preventDefault(); return; }
  if (event.detail >= 3) { event.preventDefault(); selectPointerUnit(true); }
});

editor.addEventListener('focus', () => scheduleOverlay());
editor.addEventListener('blur', () => scheduleOverlay());
document.addEventListener('selectionchange', selectionchange);
editor.addEventListener('contextmenu', () => { nativeSelectionActive = true; scheduleOverlay(); });
editor.addEventListener('beforeinput', event => {
  if (composing || event.isComposing) return;
  // Some mobile IMEs emit a final insertion after compositionend. The commit
  // already owns that text; accepting the second event duplicates it.
  if (compositionTail && performance.now() < compositionTail.until &&
      ['insertText', 'insertFromComposition'].includes(event.inputType) && event.data === compositionTail.text) {
    event.preventDefault(); compositionTail = null; return;
  }
  compositionTail = null;
  let selection = readSelection();
  const target = event.getTargetRanges?.()[0];
  if (target) {
    const anchor = endpointFromDom(target.startContainer, target.startOffset);
    const focus = endpointFromDom(target.endContainer, target.endOffset);
    if (anchor && focus) selection = {anchor, focus};
  }
  if (event.inputType === 'insertText' || event.inputType === 'insertFromComposition' || event.inputType === 'insertReplacementText') {
    event.preventDefault();
    if (!pendingInsert) pendingInsertSelection = selection;
    pendingInsert += event.data || '';
    clearTimeout(insertTimer);
    insertTimer = setTimeout(flushPendingInsert, 8);
  } else if (event.inputType === 'deleteContentBackward') {
    event.preventDefault(); flushPendingInsert(); action('delete_backward', { selection });
  } else if (event.inputType === 'deleteContentForward') {
    event.preventDefault(); flushPendingInsert(); action('delete_forward', { selection });
  } else if (event.inputType === 'historyUndo' || event.inputType === 'historyRedo') {
    event.preventDefault(); flushPendingInsert(); action(event.inputType === 'historyUndo' ? 'undo' : 'redo');
  } else if (event.inputType === 'insertParagraph' || event.inputType === 'insertLineBreak') {
    event.preventDefault(); flushPendingInsert(); action('split_block', { selection });
  }
});

editor.addEventListener('keydown', event => {
  const mod = event.metaKey || event.ctrlKey;
  if ((event.altKey || event.ctrlKey) && ['ArrowLeft', 'ArrowRight'].includes(event.key)) { nativeSelectionGuard = 0; return; }
  if (!composing && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    flushPendingInsert();
    let key = event.key;
    if (event.metaKey && key === 'ArrowLeft') key = 'Home';
    if (event.metaKey && key === 'ArrowRight') key = 'End';
    if ((event.metaKey && key === 'ArrowUp') || (event.ctrlKey && key === 'Home')) key = 'DocumentStart';
    if ((event.metaKey && key === 'ArrowDown') || (event.ctrlKey && key === 'End')) key = 'DocumentEnd';
    const started = performance.now();
    if (composing || mutationPending || !sceneMatches(layoutSnapshot, documentKey)) deferredInputs.push(() => navigateLocal(key, event.shiftKey, started));
    else navigateLocal(key, event.shiftKey, started);
    return;
  }
  if (mod && ['b', 'i', 'u', 'z', 'y'].includes(event.key.toLowerCase())) flushPendingInsert();
  if (mod && event.key.toLowerCase() === 'b') { event.preventDefault(); action('toggle_mark', { mark: 'bold' }); }
  else if (mod && event.key.toLowerCase() === 'i') { event.preventDefault(); action('toggle_mark', { mark: 'italic' }); }
  else if (mod && event.key.toLowerCase() === 'u') { event.preventDefault(); action('toggle_mark', { mark: 'underline' }); }
  else if (mod && event.key.toLowerCase() === 'z') { event.preventDefault(); action(event.shiftKey ? 'redo' : 'undo'); }
  else if (mod && event.key.toLowerCase() === 'y') { event.preventDefault(); action('redo'); }
});

editor.addEventListener('paste', event => {
  event.preventDefault();
  flushPendingInsert();
  const text = event.clipboardData?.getData('text/plain') || '';
  action('insert_text', { text });
});
editor.addEventListener('copy', async event => {
  const selection = readSelection();
  if (!selection) return;
  event.preventDefault();
  const text = window.getSelection()?.toString() || '';
  event.clipboardData?.setData('text/plain', text);
});
editor.addEventListener('cut', event => {
  const selection = readSelection();
  if (!selection || (selection.anchor.block_index === selection.focus.block_index && selection.anchor.offset === selection.focus.offset)) return;
  event.preventDefault();
  const text = window.getSelection()?.toString() || '';
  event.clipboardData?.setData('text/plain', text);
  action('insert_text', { text: '', selection });
});

editor.addEventListener('compositionstart', () => {
  if (composing) return;
  flushPendingInsert();
  compositionSelection = readSelection();
  compositionMarks = typingMarks(currentState, compositionSelection);
  compositionDirty = false;
  compositionEpoch += 1;
  compositionDocument = structuredClone(currentState.document);
  previewDocument = compositionDocument;
  compositionSnapshot = currentState.document.blocks.map(block => (block.content || []).map(run => run.text || '').join(''));
  composing = true;
  surface.classList.add('composing');
  applyCompositionStyle();
  scheduleOverlay();
  action('composition_begin', { selection: compositionSelection, keepDom: true });
});
editor.addEventListener('compositionupdate', event => {
  if (!composing) return;
  preview.textContent = event.data || '';
  compositionDirty = true;
  applyCompositionStyle();
  scheduleCompositionStyle();
  scheduleOverlay();
  // Preedit is rendered separately; queueing every keyboard update over LAN
  // delays the final commit and adds no document/history state.
});
editor.addEventListener('compositionend', event => {
  if (!composing) return;
  // Mobile keyboards can recompose the word around a moved caret. event.data
  // is the complete composing word, not necessarily text to insert. Diff the
  // native result against the engine snapshot to obtain the replacement range.
  const edits = [];
  for (let index = 0; index < (compositionSnapshot || []).length; index++) {
    const element = editor.querySelector(`[data-block="${index}"]`);
    if (!element) continue;
    const edit = replacementDiff(compositionSnapshot[index], element.textContent || '', index);
    if (edit) edits.push(edit);
  }
  const finalNativeSelection = readNativeSelection();
  composing = false;
  nativeSelectionGuard = 0;
  // The keyboard can end an unchanged reconversion because the user moved
  // the caret. That destination must survive cancel/commit responses.
  if (edits.length <= 1 && finalNativeSelection) setLocalSelection(finalNativeSelection);
  const tail = { text: event.data || '', until: performance.now() + 100 };
  compositionTail = tail;
  // Only suppress the browser's trailing insertion in this event turn. A real
  // subsequent keystroke with the same character must never be swallowed.
  setTimeout(() => { if (compositionTail === tail) compositionTail = null; }, 0);
  compositionSelection = null;
  compositionSnapshot = null;
  compositionDocument = null; previewDocument = null;
  surface.classList.remove('composing');
  for (const property of Object.keys(typingStyle([], null))) editor.style[property] = '';
  preview.textContent = '';
  // The native IME operates within one block. Preserve its captured typing
  // marks in one engine command instead of clearing them via set_selection.
  if (edits.length === 1) action('composition_replace', { ...edits[0], preserveSelection: true });
  else {
    action('composition_cancel', { noSelection: true, keepDom: edits.length === 0, preserveSelection: edits.length === 0 });
    if (edits.length > 1) {
      status.textContent = 'Komposisi lintas paragraf belum didukung; perubahan dibatalkan.';
      renderSemanticDocument();
    }
  }
  if (!edits.length) scheduleOverlay();
  const waiters = compositionWaiters; compositionWaiters = [];
  actionQueue.then(() => { renderCanvas(true); for (const resolve of waiters) resolve(); });
});

for (const button of document.querySelectorAll('[data-mark]')) {
  button.addEventListener('pointerdown', event => event.preventDefault());
  button.addEventListener('click', async () => { await finishComposition(); flushPendingInsert(); action('toggle_mark', { mark: button.dataset.mark }); });
}
document.querySelector('#undoButton').addEventListener('pointerdown', event => event.preventDefault());
document.querySelector('#undoButton').addEventListener('click', async () => { await finishComposition(); flushPendingInsert(); action('undo'); });
document.querySelector('#redoButton').addEventListener('pointerdown', event => event.preventDefault());
document.querySelector('#redoButton').addEventListener('click', async () => { await finishComposition(); flushPendingInsert(); action('redo'); });
document.querySelector('#saveButton').addEventListener('click', async () => {
  await finishComposition();
  flushPendingInsert();
  await actionQueue;
  const response = await fetch('/api/state');
  const state = await response.json();
  const blob = new Blob([JSON.stringify(state.document, null, 2)], { type: 'application/json' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = 'openrich-document.json';
  link.click();
  URL.revokeObjectURL(link.href);
  status.textContent = 'Dokumen JSON diunduh';
});
document.querySelector('#openButton').addEventListener('click', () => fileInput.click());
fileInput.addEventListener('change', async () => {
  const file = fileInput.files?.[0];
  if (!file) return;
  try {
    const state = await post('/api/load', await file.text(), true);
    await updateState(state);
    status.textContent = `Dibuka: ${file.name}`;
  } catch (error) { status.textContent = error.message; }
  fileInput.value = '';
});

async function start() {
  try {
    await Promise.all(['OpenRich Sans','OpenRich Mono'].flatMap(family => ['400','700','italic 400','italic 700'].map(style => document.fonts.load(`${style} 16px "${family}"`))));
    const response = await fetch('/api/state');
    await updateState(await response.json());
    canvasResizeObserver = new ResizeObserver(() => renderCanvas());
    canvasResizeObserver.observe(surface);
    editor.focus();
  } catch (error) {
    loading.textContent = `Gagal terhubung: ${error.message}`;
  }
}
window.addEventListener('resize', () => renderCanvas());
start();

function currentBlockFormat() {
  return { alignment: 'left', list: 'none', checked: false, ...currentState?.document.blocks[currentState?.selection?.focus?.block_index]?.format };
}
function updateBlockToolbar() {
  const format = currentBlockFormat();
  const marks = typingMarks(currentState);
  for (const button of document.querySelectorAll('[data-mark]')) {
    const active = marks.some(mark => mark.type === button.dataset.mark);
    button.classList.toggle('active', active);
    button.setAttribute('aria-pressed', String(active));
  }
  document.querySelector('#alignment').value = format.alignment;
  for (const button of document.querySelectorAll('[data-list]')) {
    const active = button.dataset.list === format.list;
    button.classList.toggle('active', active);
    button.setAttribute('aria-pressed', String(active));
  }
  document.querySelector('#checkItem').disabled = format.list !== 'checkbox';
  document.querySelector('#checkItem').setAttribute('aria-pressed', String(format.checked));
}
for (const button of document.querySelectorAll('[data-list]')) {
  button.addEventListener('pointerdown', event => event.preventDefault());
  button.addEventListener('click', async () => {
    await finishComposition();
    flushPendingInsert();
    const format = currentBlockFormat();
    format.list = format.list === button.dataset.list ? 'none' : button.dataset.list;
    format.checked = false;
    action('set_block_format', { format });
  });
}
document.querySelector('#alignment').addEventListener('change', async event => {
  const alignment = event.target.value;
  await finishComposition();
  flushPendingInsert();
  action('set_block_format', { format: { ...currentBlockFormat(), alignment }, selection: currentState.selection });
});
document.querySelector('#checkItem').addEventListener('pointerdown', event => event.preventDefault());
document.querySelector('#checkItem').addEventListener('click', async () => {
  await finishComposition();
  flushPendingInsert();
  const format = currentBlockFormat();
  action('set_block_format', { format: { ...format, checked: !format.checked } });
});

// The Rust document owns committed text. Non-cancelable mobile input may still
// mutate contenteditable; repair that DOM instead of treating it as another edit.
editor.addEventListener('input', () => {
  if (composing) { applyCompositionStyle(); scheduleCompositionStyle(); scheduleOverlay(); return; }
  if (!composing) { renderSemanticDocument(); scheduleOverlay(); }
});
editor.addEventListener('dragstart', event => event.preventDefault());
for (const [index, handle] of selectionHandles.entries()) {
  handle.addEventListener('pointerdown', event => {
    event.preventDefault(); event.stopPropagation();
    if (composing || mutationPending || !sceneMatches(layoutSnapshot, documentKey)) return;
    touchMode = true;
    handleDrag = { id: event.pointerId, index, selection: structuredClone(currentState.selection) };
    handle.setPointerCapture(event.pointerId);
  });
  handle.addEventListener('pointermove', event => {
    if (!handleDrag || handleDrag.id !== event.pointerId) return;
    event.preventDefault(); event.stopPropagation();
    const bounds = canvas.getBoundingClientRect(), scale = layoutSnapshot.width / bounds.width;
    const position = hitTest(layoutSnapshot, (event.clientX - bounds.left) * scale, (event.clientY - bounds.top - 12) * scale);
    if (position) setLocalSelection(index === 0 ? {anchor: position, focus: handleDrag.selection.focus} : {anchor: handleDrag.selection.anchor, focus: position});
  });
  const release = event => {
    event.stopPropagation();
    if (handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
    handleDrag = null;
  };
  handle.addEventListener('pointerup', release);
  handle.addEventListener('pointercancel', release);
}

// IMEs may replace their insertion node after compositionupdate/input. Observe
// text mutations without observing our own style writes or altering live text.
new MutationObserver(() => {
  if (composing) {
    applyCompositionStyle();
    scheduleCompositionStyle();
  }
}).observe(editor, {childList: true, characterData: true, subtree: true});

function scheduleCompositionStyle() {
  if (!composing || compositionStyleFrame) return;
  compositionStyleFrame = requestAnimationFrame(() => {
    compositionStyleFrame = 0;
    if (composing) {
      applyCompositionStyle();
      const texts = compositionDocument.blocks.map((_, index) => editor.querySelector(`[data-block="${index}"]`)?.textContent);
      previewDocument = preeditDocument(compositionDocument, texts, compositionMarks);
      renderCanvas();
    }
  });
}

function applyCompositionStyle() {
  const block = currentState?.document.blocks[compositionSelection?.focus?.block_index];
  const heading = block?.type === 'heading' ? block.level : block?.format?.heading_level;
  const style = typingStyle(compositionMarks, heading);
  const native = window.getSelection();
  const node = native?.focusNode;
  const host = node?.nodeType === Node.TEXT_NODE ? node.parentElement : node;
  if (host && editor.contains(host)) {
    // Style the IME's actual insertion host; do not replace its live text nodes.
    for (const element of [host, ...host.querySelectorAll('*')]) {
      for (const [property, value] of Object.entries(style)) {
        const cssProperty = property.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
        element.style.setProperty(cssProperty, value, 'important');
      }
    }
  }
}

function finishComposition() {
  if (!composing) return Promise.resolve();
  const completed = new Promise(resolve => compositionWaiters.push(resolve));
  editor.blur();
  return completed;
}
