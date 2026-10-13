// Rs Editor web adapter: mounts the WebAssembly editor on a canvas.
//
// Rendering, layout, editing and selection all happen in Rust (Skia on
// WebGL2). This file only forwards browser events and keeps the platform
// text input in sync: an EditContext where the browser has one (Chrome,
// Edge), otherwise a hidden textarea (Safari, Firefox). The textarea is
// never used for geometry, so no font matching is needed.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const MOD_SHIFT = 1, MOD_CTRL = 2, MOD_ALT = 4, MOD_META = 8;
const LINE_HEIGHT = 40;
const TOUCH_SLOP = 8;

function mods(e) {
  return (e.shiftKey ? MOD_SHIFT : 0) | (e.ctrlKey ? MOD_CTRL : 0) |
    (e.altKey ? MOD_ALT : 0) | (e.metaKey ? MOD_META : 0);
}

function platformCode() {
  const p = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  if (/mac|iphone|ipad|ipod/i.test(p)) return 1;
  if (/win/i.test(p)) return 2;
  return 0;
}

export async function mount(container, { content, dark = false, moduleUrl = "pkg/rs-editor-web.js", input } = {}) {
  if (typeof createRsEditor === "undefined") {
    await new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = moduleUrl;
      script.onload = resolve;
      script.onerror = reject;
      document.head.append(script);
    });
  }
  const M = await createRsEditor();

  const canvas = document.createElement("canvas");
  canvas.className = "rs-editor-canvas";
  canvas.tabIndex = 0;
  canvas.style.cssText = "display:block;width:100%;height:100%;outline:none;touch-action:none;cursor:text";
  container.append(canvas);

  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false, stencil: true, depth: false, premultipliedAlpha: true });
  if (!gl) throw new Error("WebGL2 tidak tersedia");
  const glHandle = M.GL.registerContext(gl, { majorVersion: 2 });
  M.GL.makeContextCurrent(glHandle);

  // --- strings across the wasm boundary ----------------------------------
  function withString(text, fn) {
    const bytes = encoder.encode(text);
    const ptr = M._rse_alloc(bytes.length);
    M.HEAPU8.set(bytes, ptr);
    try { return fn(ptr, bytes.length); } finally { M._rse_free(ptr, bytes.length); }
  }
  function withStrings(a, b, fn) {
    return withString(a, (pa, la) => withString(b, (pb, lb) => fn(pa, la, pb, lb)));
  }
  const result = (len) => decoder.decode(M.HEAPU8.subarray(M._rse_result(app), M._rse_result(app) + len));

  // --- size ---------------------------------------------------------------
  let cssWidth = Math.max(1, container.clientWidth);
  let cssHeight = Math.max(1, container.clientHeight);
  let dpr = window.devicePixelRatio || 1;
  function applySize() {
    canvas.width = Math.round(cssWidth * dpr);
    canvas.height = Math.round(cssHeight * dpr);
  }
  applySize();

  const app = withString(JSON.stringify(content ?? null), (ptr, len) =>
    M._rse_init(cssWidth, cssHeight, dpr, platformCode(), dark ? 1 : 0, ptr, len));
  if (!app) throw new Error("Editor gagal dibuat");

  new ResizeObserver(() => {
    const w = Math.max(1, container.clientWidth), h = Math.max(1, container.clientHeight);
    const d = window.devicePixelRatio || 1;
    if (w === cssWidth && h === cssHeight && d === dpr) return;
    cssWidth = w; cssHeight = h; dpr = d;
    applySize();
    M._rse_resize(app, cssWidth, cssHeight, dpr);
    requestFrame();
  }).observe(container);

  // --- frames -------------------------------------------------------------
  let frameRequested = false;
  let timer = 0;
  const stats = { frames: 0, lastFrameMs: 0 };
  function requestFrame() {
    if (frameRequested) return;
    frameRequested = true;
    clearTimeout(timer);
    requestAnimationFrame(frame);
  }
  function frame() {
    frameRequested = false;
    const start = performance.now();
    M.GL.makeContextCurrent(glHandle);
    const next = M._rse_frame(app);
    stats.frames++;
    stats.lastFrameMs = performance.now() - start;
    if (next >= 0 && next <= 16) requestFrame();
    else if (next > 16) timer = setTimeout(requestFrame, next - 8);
  }

  // --- input element ------------------------------------------------------
  const useEditContext = input !== "textarea" && "EditContext" in window;
  let editContext = null;
  let textarea = null;
  let composing = false;
  let imeStart = -1;

  if (useEditContext) {
    editContext = new EditContext();
    canvas.editContext = editContext;
    editContext.addEventListener("textupdate", (e) => {
      withString(e.text, (ptr, len) =>
        M._rse_ime_replace(app, e.updateRangeStart, e.updateRangeEnd, ptr, len, composing ? 1 : 0));
      afterInput();
    });
    editContext.addEventListener("compositionstart", () => { composing = true; });
    editContext.addEventListener("compositionend", () => {
      composing = false;
      M._rse_finish_composition(app);
      afterInput();
    });
    editContext.addEventListener("characterboundsupdate", (e) => {
      // Approximate every character with the caret box; enough for IME
      // candidate windows to sit next to the text.
      const rect = caretClientRect();
      if (!rect) return;
      const bounds = [];
      for (let i = e.rangeStart; i < e.rangeEnd; i++) bounds.push(rect);
      editContext.updateCharacterBounds(e.rangeStart, bounds);
    });
  } else {
    textarea = document.createElement("textarea");
    textarea.setAttribute("autocapitalize", "sentences");
    textarea.setAttribute("aria-hidden", "true");
    textarea.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1em;opacity:0;padding:0;border:0;resize:none;overflow:hidden;white-space:pre;font-size:16px;caret-color:transparent";
    document.body.append(textarea);
    textarea.addEventListener("beforeinput", (e) => {
      if (e.isComposing || e.inputType === "insertCompositionText") return;
      if (e.inputType === "insertText" || e.inputType === "insertReplacementText") {
        e.preventDefault();
        const text = e.data ?? e.dataTransfer?.getData("text/plain") ?? "";
        if (text) withString(text, (ptr, len) => M._rse_insert_text(app, ptr, len));
        afterInput();
      }
    });
    textarea.addEventListener("compositionstart", () => { composing = true; });
    textarea.addEventListener("compositionupdate", (e) => {
      withString(e.data ?? "", (ptr, len) => M._rse_set_composition(app, ptr, len));
      afterInput();
    });
    textarea.addEventListener("compositionend", (e) => {
      composing = false;
      withString(e.data ?? "", (ptr, len) => M._rse_commit_composition(app, ptr, len));
      textarea.value = "";
      afterInput();
    });
    textarea.addEventListener("keydown", onKeyDown);
    textarea.addEventListener("focus", () => { M._rse_focus(app, 1); requestFrame(); });
    textarea.addEventListener("blur", () => { M._rse_focus(app, 0); requestFrame(); });
  }

  function focusInput() {
    if (textarea) textarea.focus({ preventScroll: true });
    else canvas.focus({ preventScroll: true });
  }

  function caretClientRect() {
    const state = imeState();
    if (!state.caret) return null;
    const box = canvas.getBoundingClientRect();
    const [x, y, w, h] = state.caret;
    return new DOMRect(box.left + x, box.top + y, Math.max(w, 1), h);
  }

  function imeState() {
    return JSON.parse(result(M._rse_ime_state(app)));
  }

  // Keeps the platform input in step with the document.
  function syncInput() {
    const state = imeState();
    const box = canvas.getBoundingClientRect();
    const caret = state.caret
      ? new DOMRect(box.left + state.caret[0], box.top + state.caret[1], Math.max(state.caret[2], 1), state.caret[3])
      : null;
    if (editContext) {
      if (!composing) {
        if (state.start !== imeStart || editContext.text !== state.text) {
          editContext.updateText(0, editContext.text.length, state.text);
        }
        editContext.updateSelection(state.selection[0], state.selection[1]);
      }
      editContext.updateControlBounds(box);
      if (caret) editContext.updateSelectionBounds(caret);
    } else if (textarea && caret) {
      textarea.style.left = `${caret.left}px`;
      textarea.style.top = `${caret.top}px`;
      textarea.style.height = `${caret.height}px`;
    }
    imeStart = state.start;
  }

  function applyEffects() {
    const effects = JSON.parse(result(M._rse_take_effects(app)));
    for (const effect of effects) {
      if (effect.type === "redraw") requestFrame();
      else if (effect.type === "cursor") canvas.style.cursor = effect.cursor;
      else if (effect.type === "openUrl") window.open(effect.url, "_blank", "noopener");
    }
  }

  function afterInput() {
    applyEffects();
    syncInput();
    requestFrame();
  }

  // --- keyboard -----------------------------------------------------------
  function onKeyDown(e) {
    if (e.isComposing || composing || e.keyCode === 229) return;
    const handled = withStrings(e.key, e.code, (pk, lk, pc, lc) => M._rse_key(app, pk, lk, pc, lc, mods(e)));
    if (handled) {
      e.preventDefault();
      afterInput();
    }
  }
  if (!textarea) canvas.addEventListener("keydown", onKeyDown);
  canvas.addEventListener("focus", () => { M._rse_focus(app, 1); afterInput(); });
  canvas.addEventListener("blur", () => { M._rse_focus(app, 0); requestFrame(); });

  // --- clipboard ----------------------------------------------------------
  const clipboardTarget = textarea ?? canvas;
  for (const kind of ["copy", "cut"]) {
    clipboardTarget.addEventListener(kind, (e) => {
      const copied = JSON.parse(result(M._rse_copy(app, kind === "cut" ? 1 : 0)));
      if (!copied.text && !copied.html) return;
      e.clipboardData.setData("text/plain", copied.text);
      e.clipboardData.setData("text/html", copied.html);
      e.preventDefault();
      afterInput();
    });
  }
  clipboardTarget.addEventListener("paste", (e) => {
    e.preventDefault();
    // HTML keeps formatting (and is exact for Rs Editor's own copies).
    const html = e.clipboardData.getData("text/html");
    const pasted = html && withString(html, (ptr, len) => M._rse_paste_html(app, ptr, len));
    if (!pasted) {
      const text = e.clipboardData.getData("text/plain");
      if (text) withString(text, (ptr, len) => M._rse_paste(app, ptr, len));
    }
    afterInput();
  });

  // --- pointer ------------------------------------------------------------
  let touch = null;
  function local(e) {
    const box = canvas.getBoundingClientRect();
    return [e.clientX - box.left, e.clientY - box.top];
  }
  canvas.addEventListener("pointerdown", (e) => {
    focusInput();
    canvas.setPointerCapture(e.pointerId);
    const [x, y] = local(e);
    if (e.pointerType === "touch") {
      M._rse_stop_scrolling(app);
      touch = { x, y, lastY: y, scrolling: false, samples: [[e.timeStamp, y]] };
      return;
    }
    M._rse_pointer(app, 0, x, y, e.detail || 1, mods(e));
    afterInput();
  });
  canvas.addEventListener("pointermove", (e) => {
    const [x, y] = local(e);
    if (e.pointerType === "touch" && touch) {
      if (!touch.scrolling && Math.hypot(x - touch.x, y - touch.y) > TOUCH_SLOP) touch.scrolling = true;
      if (touch.scrolling) {
        M._rse_wheel(app, touch.lastY - y, 0);
        touch.lastY = y;
        touch.samples.push([e.timeStamp, y]);
        touch.samples = touch.samples.filter(([t]) => e.timeStamp - t < 100);
        requestFrame();
      }
      return;
    }
    if (M._rse_pointer(app, 1, x, y, 1, mods(e))) requestFrame();
    applyEffects();
  });
  canvas.addEventListener("pointerup", (e) => {
    const [x, y] = local(e);
    if (e.pointerType === "touch" && touch) {
      if (touch.scrolling) {
        const s = touch.samples;
        if (s.length > 1) {
          const [t0, y0] = s[0], [t1, y1] = s[s.length - 1];
          const velocity = t1 > t0 ? ((y1 - y0) / (t1 - t0)) * 1000 : 0;
          M._rse_fling(app, -velocity);
        }
      } else {
        M._rse_pointer(app, 0, x, y, 1, 0);
        M._rse_pointer(app, 2, x, y, 1, 0);
      }
      touch = null;
      afterInput();
      return;
    }
    M._rse_pointer(app, 2, x, y, 1, mods(e));
    afterInput();
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    if (e.deltaMode === 1) M._rse_wheel(app, e.deltaY * LINE_HEIGHT, 1);
    else if (e.deltaMode === 2) M._rse_wheel(app, e.deltaY * cssHeight, 1);
    else M._rse_wheel(app, e.deltaY, 0);
    requestFrame();
  }, { passive: false });

  focusInput();
  afterInput();

  return {
    canvas,
    input: useEditContext ? "EditContext" : "textarea",
    stats,
    setDark(value) { M._rse_set_dark(app, value ? 1 : 0); requestFrame(); },
    /** Debugging: the IME block as the editor sees it. */
    debug() { return { composing, imeStart, state: imeState() }; },
    focus: focusInput,
  };
}
