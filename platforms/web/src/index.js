// @rs-editor/web: the Rs Editor on a web page.
//
// Rendering, layout, editing, selection and touch gestures all happen in
// Rust (Skia on WebGL2, compiled to WebAssembly). This module forwards
// browser events, keeps the platform text input in sync (an EditContext
// where the browser has one, otherwise a hidden textarea that is never used
// for geometry) and shows what Rust asks for: the selection menu, the
// magnifier, haptics and the on-screen keyboard.

import { SelectionUi } from "./selection-ui.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const MOD_SHIFT = 1, MOD_CTRL = 2, MOD_ALT = 4, MOD_META = 8;
const LINE_HEIGHT = 40;
const DEFAULT_WASM = new URL("../dist/rs-editor-web.js", import.meta.url).href;

function mods(e) {
  return (e.shiftKey ? MOD_SHIFT : 0) | (e.ctrlKey ? MOD_CTRL : 0) |
    (e.altKey ? MOD_ALT : 0) | (e.metaKey ? MOD_META : 0);
}

function platformCode() {
  const p = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  if (/mac|iphone|ipad|ipod/i.test(p) || /iPhone|iPad/.test(navigator.userAgent)) return 1;
  if (/win/i.test(p)) return 2;
  return 0;
}

const modules = new Map();

/** Loads the Emscripten module once per URL. */
function loadModule(url) {
  if (!modules.has(url)) {
    modules.set(url, (async () => {
      if (typeof globalThis.createRsEditor === "undefined") {
        await new Promise((resolve, reject) => {
          const script = document.createElement("script");
          script.src = url;
          script.onload = resolve;
          script.onerror = () => reject(new Error(`Rs Editor: cannot load ${url}`));
          document.head.append(script);
        });
      }
      return globalThis.createRsEditor;
    })());
  }
  return modules.get(url).then((create) => create());
}

/**
 * Creates an editor inside `element`, which it fills.
 *
 * @param {import("./index").EditorOptions} options
 * @returns {Promise<import("./index").Editor>}
 */
export async function createEditor(options) {
  const editor = new Editor(options);
  await editor._start();
  return editor;
}

class Editor {
  constructor({ element, content = "", contentType, dark = false, editable = true, pasteMarkdown = false, fonts = [], wasmUrl = DEFAULT_WASM, input } = {}) {
    if (!element) throw new Error("Rs Editor: `element` is required");
    this.element = element;
    this._options = { content, contentType, dark, editable, pasteMarkdown, wasmUrl, input };
    // Fonts to load when text needs them: { family, url, ranges }.
    this._fonts = fonts.map((font) => ({ ...font, state: "idle" }));
    this._listeners = new Map();
    this._destroyed = false;
    this.stats = { frames: 0, lastFrameMs: 0 };
    /** `commands.toggleBold()`: runs one command (Tiptap's `editor.commands`). */
    this.commands = new Proxy({}, {
      get: (_, name) => (args) => this.chain().command(String(name), args).run(),
    });
  }

  async _start() {
    const M = this._M = await loadModule(this._options.wasmUrl);
    const element = this.element;
    if (getComputedStyle(element).position === "static") element.style.position = "relative";

    const canvas = this.canvas = document.createElement("canvas");
    canvas.className = "rs-editor-canvas";
    canvas.tabIndex = 0;
    canvas.setAttribute("role", "textbox");
    canvas.setAttribute("aria-multiline", "true");
    canvas.style.cssText = "display:block;width:100%;height:100%;outline:none;touch-action:none;cursor:text;-webkit-user-select:none;user-select:none;-webkit-touch-callout:none";
    element.append(canvas);

    // The magnifier copies each frame right after it is drawn, while the
    // drawing buffer is still valid, so it need not be preserved. The
    // buffer has alpha: WebGL has no advanced blending, so Skia copies the
    // framebuffer into a texture for the selection's multiply blend, and
    // WebGL cannot copy an RGB framebuffer into an RGBA texture.
    const gl = canvas.getContext("webgl2", { alpha: true, antialias: false, stencil: true, depth: false, premultipliedAlpha: true });
    if (!gl) throw new Error("Rs Editor: WebGL2 is not available");
    this._gl = M.GL.registerContext(gl, { majorVersion: 2 });
    M.GL.makeContextCurrent(this._gl);

    this._size = this._measure();
    this._applySize();
    const { width, height, dpr } = this._size;
    const content = typeof this._options.content === "string"
      ? this._options.content
      : JSON.stringify(this._options.content);
    const markdown = this._options.contentType === "markdown" ? 1 : 0;
    this._app = this._withString(content, (ptr, len) =>
      M._rse_init(width, height, dpr, platformCode(), this._options.dark ? 1 : 0, ptr, len, markdown));
    if (!this._app) throw new Error("Rs Editor: the editor could not be created");
    if (!this._options.editable) M._rse_set_editable(this._app, 0);
    if (this._options.pasteMarkdown) M._rse_set_markdown_paste(this._app, 1);

    this._revision = M._rse_revision(this._app, 0);
    this._docRevision = M._rse_revision(this._app, 1);
    this._ui = new SelectionUi(this);
    this._setUpInput();
    this._setUpPointer();
    this._setUpViewport();
    this._afterInput();
  }

  // --- strings across the wasm boundary ------------------------------------

  _withString(text, fn) {
    const M = this._M;
    const bytes = encoder.encode(text);
    const ptr = M._rse_alloc(bytes.length);
    M.HEAPU8.set(bytes, ptr);
    try { return fn(ptr, bytes.length); } finally { M._rse_free(ptr, bytes.length); }
  }

  _withStrings(a, b, fn) {
    return this._withString(a, (pa, la) => this._withString(b, (pb, lb) => fn(pa, la, pb, lb)));
  }

  _result(len) {
    const M = this._M;
    const ptr = M._rse_result(this._app);
    return decoder.decode(M.HEAPU8.subarray(ptr, ptr + len));
  }

  _json(len) {
    return JSON.parse(this._result(len));
  }

  // --- size and frames ------------------------------------------------------

  _measure() {
    return {
      width: Math.max(1, this.element.clientWidth),
      height: Math.max(1, this.element.clientHeight),
      dpr: window.devicePixelRatio || 1,
    };
  }

  _applySize() {
    const { width, height, dpr } = this._size;
    this.canvas.width = Math.round(width * dpr);
    this.canvas.height = Math.round(height * dpr);
  }

  _requestFrame() {
    if (this._frameRequested || this._destroyed) return;
    this._frameRequested = true;
    clearTimeout(this._timer);
    requestAnimationFrame(() => this._frame());
  }

  _frame() {
    this._frameRequested = false;
    if (this._destroyed) return;
    const M = this._M;
    const start = performance.now();
    M.GL.makeContextCurrent(this._gl);
    const next = M._rse_frame(this._app);
    this.stats.frames++;
    this.stats.lastFrameMs = performance.now() - start;
    // A frame can start a long press or bring the menu back after a fling.
    this._applyEffects();
    this._ui.afterFrame();
    if (next >= 0 && next <= 16) this._requestFrame();
    else if (next > 16) this._timer = setTimeout(() => this._requestFrame(), next - 8);
  }

  _setUpViewport() {
    this._resizeObserver = new ResizeObserver(() => {
      const size = this._measure();
      const old = this._size;
      if (size.width === old.width && size.height === old.height && size.dpr === old.dpr) return;
      this._size = size;
      this._applySize();
      this._M._rse_resize(this._app, size.width, size.height, size.dpr);
      this._updateObscured();
      this._requestFrame();
    });
    this._resizeObserver.observe(this.element);
    // Where the on-screen keyboard covers the page without resizing it
    // (iOS Safari), the visual viewport shrinks instead.
    this._onViewport = () => this._updateObscured();
    window.visualViewport?.addEventListener("resize", this._onViewport);
    window.visualViewport?.addEventListener("scroll", this._onViewport);
  }

  _updateObscured() {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const box = this.canvas.getBoundingClientRect();
    const visibleBottom = viewport.offsetTop + viewport.height;
    const covered = Math.max(0, Math.min(box.height, box.bottom - visibleBottom));
    if (covered === this._covered) return;
    this._covered = covered;
    this._M._rse_set_obscured_bottom(this._app, covered);
    this._afterInput();
  }

  // --- text input -----------------------------------------------------------

  _setUpInput() {
    const M = this._M;
    const app = this._app;
    this.inputMode = this._options.input !== "textarea" && "EditContext" in window ? "EditContext" : "textarea";
    this._composing = false;
    this._imeStart = -1;
    const canvas = this.canvas;

    if (this.inputMode === "EditContext") {
      const editContext = this._editContext = new EditContext();
      canvas.editContext = editContext;
      editContext.addEventListener("textupdate", (e) => {
        this._withString(e.text, (ptr, len) =>
          M._rse_ime_replace(app, e.updateRangeStart, e.updateRangeEnd, ptr, len, this._composing ? 1 : 0));
        this._afterInput();
      });
      editContext.addEventListener("compositionstart", () => { this._composing = true; });
      editContext.addEventListener("compositionend", () => {
        this._composing = false;
        M._rse_finish_composition(app);
        this._afterInput();
      });
      editContext.addEventListener("characterboundsupdate", (e) => {
        const info = this._json(M._rse_ime_cursor_info(app, e.rangeStart, e.rangeEnd));
        const box = canvas.getBoundingClientRect();
        const fallback = this._caretClientRect();
        const bounds = info.bounds.map((r) => r
          ? new DOMRect(box.left + r[0], box.top + r[1], r[2] - r[0], r[3] - r[1])
          : fallback ?? new DOMRect());
        editContext.updateCharacterBounds(e.rangeStart, bounds);
      });
    } else {
      const textarea = this._textarea = document.createElement("textarea");
      textarea.setAttribute("autocapitalize", "sentences");
      textarea.setAttribute("aria-hidden", "true");
      textarea.tabIndex = -1;
      textarea.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1em;opacity:0;padding:0;border:0;resize:none;overflow:hidden;white-space:pre;font-size:16px;caret-color:transparent";
      document.body.append(textarea);
      textarea.addEventListener("beforeinput", (e) => {
        if (e.isComposing || e.inputType === "insertCompositionText") return;
        if (e.inputType === "insertText" || e.inputType === "insertReplacementText") {
          e.preventDefault();
          const text = e.data ?? e.dataTransfer?.getData("text/plain") ?? "";
          if (text) this._withString(text, (ptr, len) => M._rse_insert_text(app, ptr, len));
          this._afterInput();
        }
      });
      textarea.addEventListener("compositionstart", () => { this._composing = true; });
      textarea.addEventListener("compositionupdate", (e) => {
        this._withString(e.data ?? "", (ptr, len) => M._rse_set_composition(app, ptr, len));
        this._afterInput();
      });
      textarea.addEventListener("compositionend", (e) => {
        this._composing = false;
        this._withString(e.data ?? "", (ptr, len) => M._rse_commit_composition(app, ptr, len));
        textarea.value = "";
        this._afterInput();
      });
      textarea.addEventListener("keydown", (e) => this._onKeyDown(e));
      textarea.addEventListener("focus", () => this._focusChanged(true));
      textarea.addEventListener("blur", () => this._focusChanged(false));
    }

    if (!this._textarea) {
      canvas.addEventListener("keydown", (e) => this._onKeyDown(e));
      canvas.addEventListener("focus", () => this._focusChanged(true));
      canvas.addEventListener("blur", () => this._focusChanged(false));
    }

    // Clipboard shortcuts and the browser's own menus.
    const target = this._textarea ?? canvas;
    for (const kind of ["copy", "cut"]) {
      target.addEventListener(kind, (e) => {
        const copied = this._json(M._rse_copy(app, kind === "cut" ? 1 : 0));
        if (!copied.text && !copied.html) return;
        e.clipboardData.setData("text/plain", copied.text);
        e.clipboardData.setData("text/html", copied.html);
        e.preventDefault();
        this._afterInput();
      });
    }
    target.addEventListener("paste", (e) => {
      e.preventDefault();
      this._paste(e.clipboardData.getData("text/plain"), e.clipboardData.getData("text/html"));
    });
  }

  _focusChanged(focused) {
    this._M._rse_focus(this._app, focused ? 1 : 0);
    this._emit(focused ? "focus" : "blur", {});
    if (focused) this._afterInput();
    else this._requestFrame();
  }

  _onKeyDown(e) {
    if (e.isComposing || this._composing || e.keyCode === 229) return;
    const handled = this._withStrings(e.key, e.code, (pk, lk, pc, lc) =>
      this._M._rse_key(this._app, pk, lk, pc, lc, mods(e)));
    if (handled) {
      e.preventDefault();
      this._afterInput();
    }
  }

  /** Pastes clipboard content: HTML keeps formatting (exact for Rs Editor's own copies). */
  _paste(text, html) {
    const M = this._M;
    const pasted = html && this._withString(html, (ptr, len) => M._rse_paste_html(this._app, ptr, len));
    if (!pasted && text) this._withString(text, (ptr, len) => M._rse_paste(this._app, ptr, len));
    this._afterInput();
  }

  _focusInput() {
    if (this._textarea) this._textarea.focus({ preventScroll: true });
    else this.canvas.focus({ preventScroll: true });
  }

  _imeState() {
    return this._json(this._M._rse_ime_state(this._app));
  }

  _caretClientRect() {
    const state = this._imeState();
    if (!state.caret) return null;
    const box = this.canvas.getBoundingClientRect();
    const [x, y, w, h] = state.caret;
    return new DOMRect(box.left + x, box.top + y, Math.max(w, 1), h);
  }

  /** Keeps the platform input in step with the document. */
  _syncInput() {
    const state = this._imeState();
    const box = this.canvas.getBoundingClientRect();
    const caret = state.caret
      ? new DOMRect(box.left + state.caret[0], box.top + state.caret[1], Math.max(state.caret[2], 1), state.caret[3])
      : null;
    const editContext = this._editContext;
    if (editContext) {
      if (!this._composing) {
        if (state.start !== this._imeStart || editContext.text !== state.text) {
          editContext.updateText(0, editContext.text.length, state.text);
        }
        editContext.updateSelection(state.selection[0], state.selection[1]);
      }
      editContext.updateControlBounds(box);
      if (caret) editContext.updateSelectionBounds(caret);
    } else if (this._textarea && caret) {
      const style = this._textarea.style;
      style.left = `${caret.left}px`;
      style.top = `${caret.top}px`;
      style.height = `${caret.height}px`;
    }
    this._imeStart = state.start;
  }

  _applyEffects() {
    const effects = this._json(this._M._rse_take_effects(this._app));
    for (const effect of effects) {
      switch (effect.type) {
        case "redraw": this._requestFrame(); break;
        case "cursor": this.canvas.style.cursor = effect.cursor; break;
        case "openUrl": window.open(effect.url, "_blank", "noopener"); break;
        case "showKeyboard": this._focusInput(); break;
        case "clipboard": this._ui.writeClipboard(effect.text, effect.html); break;
        case "haptic": this._ui.haptic(effect.kind); break;
        case "menu": this._ui.showMenu(effect.menu); break;
        case "magnifier": this._ui.showMagnifier(effect.at); break;
        case "missingCharacters": this._loadFontsFor(effect.characters); break;
      }
    }
  }

  _afterInput() {
    if (this._destroyed) return;
    this._applyEffects();
    this._syncInput();
    this._requestFrame();
    this._notify();
  }

  _notify() {
    const M = this._M;
    const revision = M._rse_revision(this._app, 0);
    if (revision === this._revision) return;
    this._revision = revision;
    const docRevision = M._rse_revision(this._app, 1);
    const docChanged = docRevision !== this._docRevision;
    this._docRevision = docRevision;
    this._emit("transaction", { editor: this });
    this._emit(docChanged ? "update" : "selectionUpdate", { editor: this });
  }

  // --- fonts ------------------------------------------------------------------

  /** Loads the configured fonts that cover `characters` (code points). */
  _loadFontsFor(characters) {
    for (const font of this._fonts) {
      if (font.state !== "idle") continue;
      const covers = !font.ranges || characters.some((c) =>
        font.ranges.some((range) => Array.isArray(range) ? c >= range[0] && c <= range[1] : c === range));
      if (!covers) continue;
      font.state = "loading";
      fetch(font.url)
        .then((response) => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return response.arrayBuffer();
        })
        .then((bytes) => {
          font.state = this.addFont(bytes, font.family) ? "loaded" : "failed";
        })
        .catch((error) => {
          font.state = "failed";
          console.warn(`Rs Editor: font ${font.url} could not be loaded`, error);
        });
    }
  }

  /**
   * Adds a font (TTF/OTF bytes) for characters the bundled fonts lack, under
   * `family` or the font's own name; returns the family, or "" when the bytes
   * are not a font.
   */
  addFont(bytes, family = "") {
    if (this._destroyed) return "";
    const M = this._M;
    const data = new Uint8Array(bytes);
    const ptr = M._rse_alloc(data.length);
    M.HEAPU8.set(data, ptr);
    let added;
    try {
      added = this._withString(family, (fp, fl) =>
        this._result(M._rse_add_font(this._app, ptr, data.length, fp, fl)));
    } finally {
      M._rse_free(ptr, data.length);
    }
    this._afterInput();
    return added;
  }

  // --- pointer and touch ----------------------------------------------------

  _setUpPointer() {
    const M = this._M;
    const app = this._app;
    const canvas = this.canvas;
    const local = (e) => {
      const box = canvas.getBoundingClientRect();
      return [e.clientX - box.left, e.clientY - box.top];
    };
    let touchId = null;
    // Pointer events need not carry the click count (`detail` may be 0), so
    // count double and triple clicks here as well.
    let lastClick = { time: 0, x: 0, y: 0, count: 0 };
    const clickCount = (e, x, y) => {
      if (e.detail > 0) return e.detail;
      const near = e.timeStamp - lastClick.time < 500 && Math.hypot(x - lastClick.x, y - lastClick.y) < 5;
      lastClick = { time: e.timeStamp, x, y, count: near ? lastClick.count % 3 + 1 : 1 };
      return lastClick.count;
    };
    canvas.addEventListener("pointerdown", (e) => {
      const [x, y] = local(e);
      if (e.pointerType === "touch") {
        // One finger drives the editor; Rust recognizes taps, long
        // presses, handles and scrolling, and asks for the keyboard.
        if (touchId !== null) return;
        touchId = e.pointerId;
        canvas.setPointerCapture(e.pointerId);
        M._rse_touch(app, 0, x, y);
        this._afterInput();
        return;
      }
      // Only the primary button edits; the secondary one opens the menu.
      if (e.button !== 0) return;
      this._ui.showMenu(null);
      this._focusInput();
      canvas.setPointerCapture(e.pointerId);
      M._rse_pointer(app, 0, x, y, clickCount(e, x, y), mods(e));
      this._afterInput();
    });
    canvas.addEventListener("pointermove", (e) => {
      const [x, y] = local(e);
      if (e.pointerType === "touch") {
        if (e.pointerId !== touchId) return;
        // Coalesced events keep handle drags and flings precise.
        for (const event of e.getCoalescedEvents?.() ?? [e]) {
          const [cx, cy] = local(event);
          M._rse_touch(app, 1, cx, cy);
        }
        this._afterInput();
        return;
      }
      if (M._rse_pointer(app, 1, x, y, 1, mods(e))) this._requestFrame();
      this._applyEffects();
    });
    const end = (e, action) => {
      const [x, y] = local(e);
      if (e.pointerType === "touch") {
        if (e.pointerId !== touchId) return;
        touchId = null;
        // Keyboard requests must be handled inside this event to count as
        // a user gesture.
        M._rse_touch(app, action, x, y);
        this._afterInput();
        return;
      }
      if (e.button !== 0) return;
      M._rse_pointer(app, 2, x, y, 1, mods(e));
      this._afterInput();
    };
    canvas.addEventListener("pointerup", (e) => end(e, 2));
    canvas.addEventListener("pointercancel", (e) => end(e, 3));
    canvas.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      // The same menu as for touch, at the mouse (touch menus come from Rust).
      if (touchId !== null || e.pointerType === "touch") return;
      const [x, y] = local(e);
      this._focusInput();
      const items = this.selection.empty ? ["paste", "selectAll"] : ["cut", "copy", "paste", "selectAll"];
      this._ui.showMenu({ rect: [x, y, x, y], items }, { atPoint: true });
    });
    // The browser's own touch gestures (its long press on an editable
    // element cancels the pointer) would fight the ones Rust recognizes.
    canvas.addEventListener("touchstart", (e) => e.preventDefault(), { passive: false });
    canvas.addEventListener("wheel", (e) => {
      e.preventDefault();
      if (e.deltaMode === 1) M._rse_wheel(app, e.deltaY * LINE_HEIGHT, 1);
      else if (e.deltaMode === 2) M._rse_wheel(app, e.deltaY * this._size.height, 1);
      else M._rse_wheel(app, e.deltaY, 0);
      this._requestFrame();
    }, { passive: false });
  }

  // --- public API -------------------------------------------------------------

  /** Commands to run as one transaction (one undo step): `chain().toggleBold().run()`. */
  chain() { return chainOf(this, false); }

  /** Whether a chain would apply, without changing anything: `can().toggleBold().run()`. */
  can() { return chainOf(this, true); }

  _runSteps(steps, dryRun, focus) {
    if (this._destroyed) return false;
    const ok = this._withString(JSON.stringify(steps), (ptr, len) =>
      this._M._rse_chain(this._app, ptr, len, dryRun ? 1 : 0)) === 1;
    if (!dryRun) {
      if (focus) this._focusInput();
      this._afterInput();
    }
    return ok;
  }

  /** The selection: `{ anchor, head, from, to, empty }` in document positions. */
  get selection() { return this._json(this._M._rse_selection(this._app)); }

  /** Whether the mark or node `name` (with `attrs`, if given) is active at the selection. */
  isActive(name, attrs = {}) {
    return this._withStrings(name, JSON.stringify(attrs), (pn, ln, pa, la) =>
      this._M._rse_is_active(this._app, pn, ln, pa, la)) === 1;
  }

  /** Attributes of the mark or node `name` at the selection. */
  getAttributes(name) {
    return JSON.parse(this._withString(name, (ptr, len) =>
      this._result(this._M._rse_get_attributes(this._app, ptr, len))));
  }

  getJSON() { return this._json(this._M._rse_get_content(this._app, 0)); }
  getHTML() { return this._result(this._M._rse_get_content(this._app, 1)); }
  getText() { return this._result(this._M._rse_get_content(this._app, 2)); }
  /** The document as GitHub-flavored Markdown. */
  getMarkdown() { return this._result(this._M._rse_get_content(this._app, 3)); }
  get isEmpty() { return this._M._rse_is_empty(this._app) === 1; }

  /**
   * Replaces the document with Tiptap JSON (an object), HTML (a string) or,
   * with `{ contentType: "markdown" }`, Markdown.
   */
  setContent(content, { contentType } = {}) {
    const json = typeof content !== "string";
    const format = json ? 0 : contentType === "markdown" ? 2 : 1;
    const text = json ? JSON.stringify(content) : content;
    const ok = this._withString(text, (ptr, len) =>
      this._M._rse_set_content(this._app, ptr, len, format)) === 1;
    this._afterInput();
    return ok;
  }

  /** Whether pasted plain text is read as Markdown. */
  setPasteMarkdown(markdown) {
    this._M._rse_set_markdown_paste(this._app, markdown ? 1 : 0);
  }

  get isEditable() { return this._M._rse_is_editable(this._app) === 1; }

  setEditable(editable) {
    this._M._rse_set_editable(this._app, editable ? 1 : 0);
    if (!editable) this._ui.showMenu(null);
    this._afterInput();
  }

  setDark(dark) {
    this._M._rse_set_dark(this._app, dark ? 1 : 0);
    this._ui.setDark(dark);
    this._requestFrame();
  }

  focus() {
    this._focusInput();
    this._afterInput();
  }

  /** Events: `transaction`, `update` (document changed), `selectionUpdate`, `focus`, `blur`. */
  on(event, callback) {
    if (!this._listeners.has(event)) this._listeners.set(event, new Set());
    this._listeners.get(event).add(callback);
    return this;
  }

  off(event, callback) {
    this._listeners.get(event)?.delete(callback);
    return this;
  }

  _emit(event, payload) {
    for (const callback of this._listeners.get(event) ?? []) callback(payload);
  }

  /** Frees the editor and removes its elements. */
  destroy() {
    if (this._destroyed) return;
    this._destroyed = true;
    clearTimeout(this._timer);
    this._resizeObserver?.disconnect();
    window.visualViewport?.removeEventListener("resize", this._onViewport);
    window.visualViewport?.removeEventListener("scroll", this._onViewport);
    this._ui.destroy();
    this._textarea?.remove();
    const M = this._M;
    M.GL.makeContextCurrent(this._gl);
    M._rse_destroy(this._app);
    M.GL.deleteContext(this._gl);
    this.canvas.remove();
    this._listeners.clear();
  }

  /** Debugging: the IME block as the editor sees it. */
  debug() {
    return { composing: this._composing, imeStart: this._imeStart, state: this._imeState() };
  }
}

/**
 * Noto fonts for emoji and CJK on public CDNs, to pass as `fonts` when the
 * page may fetch them from there (they are large: self-host them for
 * production). The bundled fonts cover Latin, Arabic and Devanagari.
 */
export const notoFonts = {
  emoji: {
    family: "Noto Color Emoji",
    url: "https://cdn.jsdelivr.net/npm/noto-color-emoji@1.0.1/ttf/NotoColorEmoji.ttf",
    ranges: [[0x00a9, 0x00a9], [0x00ae, 0x00ae], [0x203c, 0x2049], [0x2122, 0x2139], [0x2194, 0x21aa],
      [0x231a, 0x23ff], [0x24c2, 0x24c2], [0x25aa, 0x27bf], [0x2934, 0x2935], [0x2b05, 0x2b55],
      [0x3030, 0x303d], [0x3297, 0x3299], [0x1f000, 0x1faff]],
  },
  japanese: {
    family: "Noto Sans JP",
    url: "https://fonts.gstatic.com/s/notosansjp/v57/-F6jfjtqLzI2JPCgQBnw7HFyzSD-AsregP8VFBEj75vY0rw_mMI.ttf",
    ranges: [[0x3000, 0x30ff], [0x31f0, 0x31ff], [0x3400, 0x4dbf], [0x4e00, 0x9fff], [0xf900, 0xfaff], [0xff00, 0xffef]],
  },
  korean: {
    family: "Noto Sans KR",
    url: "https://fonts.gstatic.com/s/notosanskr/v40/PbyxFmXiEBPT4ITbgNA5Cgms3VYcOA-vvnIzzuoyeLTq8H4gReI.ttf",
    ranges: [[0x1100, 0x11ff], [0x3130, 0x318f], [0xac00, 0xd7af]],
  },
};

/**
 * A chain of commands. Any command name works as a method
 * (`toggleHeading({ level: 2 })`), as in Tiptap.
 */
function chainOf(editor, dryRun) {
  const steps = [];
  let focus = false;
  const chain = new Proxy({}, {
    get(_, name) {
      if (name === "run") return () => editor._runSteps(steps, dryRun, focus);
      if (name === "focus") return () => { focus = true; return chain; };
      if (name === "command") {
        return (command, args) => { steps.push({ name: command, args: args ?? null }); return chain; };
      }
      if (typeof name !== "string" || name === "then") return undefined;
      return (args) => { steps.push({ name, args: args ?? null }); return chain; };
    },
  });
  return chain;
}
