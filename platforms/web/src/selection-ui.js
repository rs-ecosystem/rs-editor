// The platform side of touch selection on the web: the floating selection
// menu, the magnifier and haptics. Rust decides when; this only shows them,
// in the shadcn/ui style of the editor (design/tokens.json).

const LABELS = {
  id: { cut: "Potong", copy: "Salin", paste: "Tempel", selectAll: "Pilih semua" },
  en: { cut: "Cut", copy: "Copy", paste: "Paste", selectAll: "Select all" },
};

const THEMES = {
  light: { popover: "#FFFFFF", foreground: "#0A0A0A", border: "#E5E5E5", accent: "#F5F5F5", shadow: "0 4px 16px rgb(0 0 0 / 0.12)" },
  dark: { popover: "#171717", foreground: "#FAFAFA", border: "rgb(255 255 255 / 0.1)", accent: "#262626", shadow: "0 4px 16px rgb(0 0 0 / 0.5)" },
};

const MAGNIFIER = { width: 120, height: 48, zoom: 1.25, lift: 44 };

/** Labels in the editor's UI language (its `locale`; "in" is Indonesian too). */
function labels(locale) {
  const language = (locale || navigator.language || "en").slice(0, 2).toLowerCase();
  return LABELS[language === "in" ? "id" : language] ?? LABELS.en;
}

export class SelectionUi {
  constructor(editor) {
    this.editor = editor;
    this.theme = THEMES.light;

    const menu = this.menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    menu.className = "rs-editor-menu";
    menu.style.cssText = "position:absolute;z-index:10;display:none;gap:2px;padding:4px;border-radius:10px;font:500 14px/1 ui-sans-serif,system-ui,-apple-system,'Segoe UI',sans-serif;white-space:nowrap;-webkit-user-select:none;user-select:none;touch-action:none";
    // Keep the editor focused (and the keyboard up) when the menu is used.
    for (const type of ["pointerdown", "mousedown", "touchstart"]) {
      menu.addEventListener(type, (e) => { e.preventDefault(); e.stopPropagation(); }, { passive: false });
    }
    editor.element.append(menu);
    // A press anywhere else, or Escape, closes the menu.
    this._onOutside = (e) => {
      if (this.menu.style.display !== "none" && !this.menu.contains(e.target) && e.target !== editor.canvas) {
        this.dismiss();
      }
    };
    this._onEscape = (e) => {
      if (e.key === "Escape" && this.menu.style.display !== "none") this.dismiss();
    };
    document.addEventListener("pointerdown", this._onOutside, true);
    document.addEventListener("keydown", this._onEscape, true);

    const magnifier = this.magnifier = document.createElement("canvas");
    magnifier.className = "rs-editor-magnifier";
    magnifier.style.cssText = `display:none;position:absolute;z-index:11;width:${MAGNIFIER.width}px;height:${MAGNIFIER.height}px;border-radius:${MAGNIFIER.height / 2}px;pointer-events:none`;
    editor.element.append(magnifier);
    this.magnifierAt = null;
    this.setDark(editor._options.dark);
  }

  setDark(dark) {
    this.theme = dark ? THEMES.dark : THEMES.light;
    const t = this.theme;
    Object.assign(this.menu.style, { background: t.popover, color: t.foreground, border: `1px solid ${t.border}`, boxShadow: t.shadow });
    Object.assign(this.magnifier.style, { border: `1px solid ${t.border}`, boxShadow: t.shadow });
  }

  // --- menu -----------------------------------------------------------------

  /**
   * Shows the menu next to `menu.rect` ([left, top, right, bottom]), or hides it
   * (null). `atPoint` opens it below and right of the rect's corner, as a
   * context menu.
   */
  showMenu(menu, { atPoint = false } = {}) {
    if (!menu || !menu.items.length) {
      this.menu.style.display = "none";
      return;
    }
    const text = labels(this.editor._options.locale);
    this.menu.replaceChildren(...menu.items.map((item) => {
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("role", "menuitem");
      button.textContent = text[item] ?? item;
      button.style.cssText = "font:inherit;color:inherit;background:none;border:0;border-radius:6px;padding:10px 12px;cursor:pointer";
      button.addEventListener("pointerenter", () => { button.style.background = this.theme.accent; });
      button.addEventListener("pointerleave", () => { button.style.background = "none"; });
      button.addEventListener("click", (e) => {
        e.preventDefault();
        this.menuAction(item);
      });
      return button;
    }));
    this.menu.style.display = "flex";
    const [left, top, right, bottom] = menu.rect;
    const width = this.menu.offsetWidth;
    const height = this.menu.offsetHeight;
    const area = this.editor.element;
    const gap = 8;
    let x, y;
    if (atPoint) {
      x = left + 2;
      y = top + 2;
      if (y + height > area.clientHeight - 4) y = top - height - 2;
    } else {
      // Above the selection; below it (clear of the handles) when there is no room.
      y = top - gap - height;
      if (y < 4) y = bottom + gap + 28;
      x = (left + right) / 2 - width / 2;
    }
    y = Math.max(4, Math.min(y, area.clientHeight - height - 4));
    x = Math.max(4, Math.min(x, area.clientWidth - width - 4));
    this.menu.style.left = `${x}px`;
    this.menu.style.top = `${y}px`;
  }

  /** Closes the menu on the user's behalf and tells the editor. */
  dismiss() {
    this.showMenu(null);
    this.editor._M._rse_menu_dismissed(this.editor._app);
    this.editor._requestFrame();
  }

  async menuAction(item) {
    const editor = this.editor;
    const M = editor._M;
    let text = null, html = null;
    if (item === "paste") {
      ({ text, html } = await readClipboard());
      if (text === null && html === null) return;
    }
    if (item === "cut") {
      // The selection goes only once the clipboard holds it: without a
      // clipboard (plain HTTP) or permission, Cut must not lose the text.
      const copied = editor._json(M._rse_copy(editor._app, 0));
      if (!(await this.writeClipboard(copied.text, copied.html))) {
        this.haptic("reject");
        return;
      }
    }
    const strings = [item, text, html].map((value) => value === null ? null : new TextEncoder().encode(value));
    const pointers = strings.map((bytes) => {
      if (!bytes) return 0;
      const ptr = M._rse_alloc(bytes.length);
      M.HEAPU8.set(bytes, ptr);
      return ptr;
    });
    let result;
    try {
      const len = M._rse_menu_action(editor._app,
        pointers[0], strings[0].length,
        pointers[1], strings[1]?.length ?? 0,
        pointers[2], strings[2]?.length ?? 0);
      result = editor._result(len);
    } finally {
      pointers.forEach((ptr, i) => { if (ptr) M._rse_free(ptr, strings[i].length); });
    }
    if (result && item !== "cut") {
      const copied = JSON.parse(result);
      this.writeClipboard(copied.text, copied.html);
    }
    editor._afterInput();
  }

  /**
   * Puts copied content on the clipboard, as HTML with a plain text
   * fallback. Resolves to whether the clipboard took it.
   */
  async writeClipboard(text, html) {
    if (!text && !html) return false;
    const clipboard = navigator.clipboard;
    if (!clipboard) return false;
    if (html && clipboard.write && typeof ClipboardItem !== "undefined") {
      try {
        await clipboard.write([new ClipboardItem({
          "text/plain": new Blob([text], { type: "text/plain" }),
          "text/html": new Blob([html], { type: "text/html" }),
        })]);
        return true;
      } catch {
        // Fall back to plain text.
      }
    }
    try {
      await clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }

  // --- magnifier ------------------------------------------------------------

  /** Magnifies around `at` ([x, y] in the canvas), or hides (null). */
  showMagnifier(at) {
    this.magnifierAt = at;
    this.magnifier.style.display = at ? "block" : "none";
    if (at) this.drawMagnifier();
  }

  afterFrame() {
    if (this.magnifierAt) this.drawMagnifier();
  }

  drawMagnifier() {
    const [x, y] = this.magnifierAt;
    const source = this.editor.canvas;
    const dpr = this.editor._size.dpr;
    const { width, height, zoom, lift } = MAGNIFIER;
    const view = this.magnifier;
    if (view.width !== Math.round(width * dpr)) {
      view.width = Math.round(width * dpr);
      view.height = Math.round(height * dpr);
    }
    const sw = (width / zoom) * dpr;
    const sh = (height / zoom) * dpr;
    const context = view.getContext("2d");
    context.fillStyle = this.theme.popover;
    context.fillRect(0, 0, view.width, view.height);
    context.drawImage(source, x * dpr - sw / 2, y * dpr - sh / 2, sw, sh, 0, 0, view.width, view.height);
    const area = this.editor.element;
    const left = Math.max(0, Math.min(x - width / 2, area.clientWidth - width));
    view.style.left = `${left}px`;
    view.style.top = `${Math.max(0, y - lift - height)}px`;
  }

  // --- haptics --------------------------------------------------------------

  haptic(kind) {
    // Android browsers only (iOS Safari has no vibration API). Pulses
    // shorter than about 10 ms are too weak to feel on many phones; Chrome
    // vibrates only after the page had a user gesture, and silent mode may
    // mute it.
    navigator.vibrate?.(kind === "longPress" ? 25 : kind === "reject" ? [12, 40, 12] : 10);
  }

  destroy() {
    document.removeEventListener("pointerdown", this._onOutside, true);
    document.removeEventListener("keydown", this._onEscape, true);
    this.menu.remove();
    this.magnifier.remove();
  }
}

/** The clipboard's text and HTML (null where missing or not allowed). */
async function readClipboard() {
  const clipboard = navigator.clipboard;
  let text = null, html = null;
  try {
    if (clipboard?.read) {
      for (const item of await clipboard.read()) {
        if (html === null && item.types.includes("text/html")) html = await (await item.getType("text/html")).text();
        if (text === null && item.types.includes("text/plain")) text = await (await item.getType("text/plain")).text();
      }
      return { text, html };
    }
  } catch {
    // Fall back to plain text.
  }
  try {
    text = await clipboard?.readText() ?? null;
  } catch {
    // Not allowed.
  }
  return { text, html };
}
