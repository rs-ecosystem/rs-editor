// Accessibility on the web: the editor draws into a canvas, so screen
// readers get an invisible DOM mirror of the same accessibility tree that
// AccessKit gives native screen readers (headings, paragraphs, lists,
// checkboxes, one span per line, each at its place on screen), and spoken
// feedback for caret moves through a live region.

const ROLE_ELEMENTS = {
  Paragraph: "p",
  Blockquote: "blockquote",
  Code: "pre",
  List: "ul",
  ListItem: "li",
  Splitter: "hr",
};

const LABELS = {
  id: { editor: "Editor teks", selected: "dipilih", characters: "karakter dipilih", blank: "kosong", newline: "baris baru", space: "spasi" },
  en: { editor: "Text editor", selected: "selected", characters: "characters selected", blank: "blank", newline: "new line", space: "space" },
};

function labels() {
  return LABELS[(navigator.language || "en").slice(0, 2)] ?? LABELS.en;
}

/** Hidden from sight, not from assistive technology. */
const VISUALLY_HIDDEN = "position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap;border:0";

export class AccessibilityMirror {
  constructor(editor) {
    this.editor = editor;
    /** Node data by id, as last received. */
    this.nodes = new Map();
    this.elements = new Map();
    this.parents = new Map();
    this.selection = null;
    this.full = true;

    const root = this.root = document.createElement("div");
    root.className = "rs-editor-a11y";
    root.setAttribute("role", "document");
    root.setAttribute("aria-label", labels().editor);
    // Laid over the canvas so screen readers highlight the right place;
    // invisible and never in the way of the pointer.
    root.style.cssText = "position:absolute;inset:0;overflow:hidden;pointer-events:none;color:transparent;-webkit-user-select:none;user-select:none;z-index:0";
    editor.element.append(root);

    const live = this.live = document.createElement("div");
    live.setAttribute("aria-live", "polite");
    live.setAttribute("aria-atomic", "true");
    live.style.cssText = VISUALLY_HIDDEN;
    editor.element.append(live);

    editor.canvas.setAttribute("aria-label", labels().editor);
    editor._textarea?.setAttribute("aria-label", labels().editor);
    this.scheduled = false;
  }

  /** Brings the mirror up to date soon (at most once per idle period). */
  schedule() {
    if (this.scheduled) return;
    this.scheduled = true;
    const run = () => {
      this.scheduled = false;
      this.update();
    };
    if ("requestIdleCallback" in window) requestIdleCallback(run, { timeout: 300 });
    else setTimeout(run, 100);
  }

  /**
   * Applies the nodes that changed since the last update (everything the
   * first time): only their elements, and their children's places, change.
   */
  update() {
    const editor = this.editor;
    if (editor._destroyed) return;
    const len = editor._M._rse_accessibility_update(editor._app, this.full ? 1 : 0);
    this.full = false;
    if (!len) return;
    const update = editor._json(len);
    this.rootId = update.root;
    if (update.selection) this.selection = update.selection;
    const changed = update.nodes.map((node) => node.id);
    const previous = new Map(changed.map((id) => [id, this.nodes.get(id)]));
    for (const node of update.nodes) this.nodes.set(node.id, node);
    for (const id of changed) this.render(id, previous.get(id));
  }

  /** The element for node `id`, created (with its subtree) when missing. */
  element(id) {
    if (id === this.rootId) return this.root;
    let element = this.elements.get(id);
    if (!element && this.nodes.has(id)) {
      this.render(id, undefined);
      element = this.elements.get(id);
    }
    return element;
  }

  /** Updates node `id`'s element and children (`before`: its old data). */
  render(id, before) {
    const node = this.nodes.get(id);
    if (!node) return;
    let element = id === this.rootId ? this.root : this.elements.get(id);
    const tag = tagOf(node);
    if (!element || (element !== this.root && element.localName !== tag)) {
      const old = element;
      element = createElement(node, tag, (target, action) => this.action(target, action));
      this.elements.set(id, element);
      old?.replaceWith(element);
    }
    element.dataset.id = id;
    if (node.role === "CheckBox") element.setAttribute("aria-checked", String(node.checked === true));
    if (node.role === "Image") element.setAttribute("aria-label", node.label ?? "");
    if (node.role === "TextRun") {
      const text = (node.text ?? "").replace(/\n$/, "");
      if (element.textContent !== text) element.textContent = text;
    } else {
      // Children sit relative to this node's box: they move only when it
      // does (not when only its transform does, as in scrolling).
      const origin = (box) => (box ? `${box[0]},${box[1]}` : "");
      const moved = !before || origin(before.bounds) !== origin(node.bounds);
      const known = new Set(before?.children ?? []);
      const children = [];
      for (const child of node.children) {
        const fresh = !this.elements.has(child);
        this.parents.set(child, id);
        const childElement = this.element(child);
        if (childElement) {
          if (moved || fresh || !known.has(child)) this.place(child);
          children.push(childElement);
        }
      }
      replaceChildren(element, children);
      const current = new Set(node.children);
      for (const old of before?.children ?? []) {
        if (!current.has(old)) this.forget(old);
      }
      labelCheckbox(node, children);
    }
    if (element !== this.root) this.place(id);
  }

  /** Positions node `id` inside its parent's box. */
  place(id) {
    const node = this.nodes.get(id);
    const element = this.elements.get(id);
    if (!node || !element) return;
    const parent = this.nodes.get(this.parents.get(id));
    const origin = parent?.bounds ?? [0, 0, 0, 0];
    const [x0, y0, x1, y1] = transformBox(node.transform, node.bounds ?? origin);
    const style = element.style;
    style.left = `${x0 - origin[0]}px`;
    style.top = `${y0 - origin[1]}px`;
    style.width = `${x1 - x0}px`;
    style.height = `${y1 - y0}px`;
    if (node.role === "TextRun") {
      style.fontSize = `${Math.max(1, (y1 - y0) * 0.75)}px`;
      style.lineHeight = `${y1 - y0}px`;
    }
  }

  /** Drops node `id` and its subtree. */
  forget(id) {
    const node = this.nodes.get(id);
    if (!node) return;
    for (const child of node.children) {
      if (this.parents.get(child) === id) this.forget(child);
    }
    this.nodes.delete(id);
    this.elements.get(id)?.remove();
    this.elements.delete(id);
    this.parents.delete(id);
  }

  action(id, action) {
    const editor = this.editor;
    const ok = editor._withStrings(id, action, (pi, li, pa, la) =>
      editor._M._rse_accessibility_action(editor._app, pi, li, pa, la)) === 1;
    if (ok) editor._afterInput();
    return ok;
  }

  /** Speaks `text` through the live region. */
  announce(text) {
    if (!text) return;
    // Clearing first makes a repeated message (the same character twice)
    // be spoken again.
    this.live.textContent = "";
    clearTimeout(this.announceTimer);
    this.announceTimer = setTimeout(() => { this.live.textContent = text; }, 30);
  }

  /**
   * After a key moved the caret or the selection, says what a text field
   * would: the character, word or line reached, or the selected text.
   */
  announceMove(event) {
    // Bring the mirror (and its idea of the selection) up to date now.
    this.update();
    const selection = this.selection;
    if (!selection) return;
    const text = labels();
    if (event.shiftKey && !this.editor.selection.empty) {
      const runs = new Map();
      collectRuns(this.nodes, this.rootId, runs);
      const selected = selectedText(runs, selection);
      this.announce(selected.length <= 80 ? `${selected} ${text.selected}` : `${selected.length} ${text.characters}`);
      return;
    }
    const [runId, index] = selection.focus;
    const run = this.nodes.get(runId);
    if (!run) return;
    const line = (run.text ?? "").replace(/\n$/, "");
    const characters = graphemes(line);
    const key = event.key;
    if (key === "ArrowUp" || key === "ArrowDown" || key === "PageUp" || key === "PageDown") {
      this.announce(line.trim() ? line : text.blank);
    } else if (key === "Home" || key === "End" || key === "ArrowLeft" || key === "ArrowRight") {
      if (event.ctrlKey || event.altKey) {
        const rest = characters.slice(index).join("");
        const word = rest.match(/^\S*/)[0] || rest.trim().split(/\s+/)[0];
        this.announce(word || text.newline);
      } else {
        const character = characters[index];
        this.announce(character === undefined ? text.newline : character === " " ? text.space : character);
      }
    }
  }

  destroy() {
    this.root.remove();
    this.live.remove();
  }
}

/**
 * The characters of `text` as AccessKit counts them (graphemes), which is
 * what text positions index.
 */
function graphemes(text) {
  if (typeof Intl !== "undefined" && Intl.Segmenter) {
    return [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text)].map((s) => s.segment);
  }
  return Array.from(text);
}

/** Text runs under node `id`, in document order. */
function collectRuns(nodes, id, runs) {
  const node = nodes.get(id);
  if (!node) return;
  if (node.role === "TextRun") runs.set(node.id, node);
  for (const child of node.children) collectRuns(nodes, child, runs);
}

function tagOf(node) {
  if (node.role === "Heading") return `h${Math.min(6, Math.max(1, node.level ?? 2))}`;
  return ROLE_ELEMENTS[node.role] ?? (node.role === "TextRun" ? "span" : "div");
}

function createElement(node, tag, action) {
  const element = document.createElement(tag);
  const style = element.style;
  style.position = "absolute";
  style.margin = "0";
  style.padding = "0";
  if (node.role === "TextRun") {
    style.whiteSpace = "pre";
    style.overflow = "hidden";
  } else if (node.role === "CheckBox") {
    element.setAttribute("role", "checkbox");
    element.tabIndex = -1;
    // Screen readers activate it with a click, which reaches the element
    // directly even though the mirror ignores the pointer.
    element.addEventListener("click", (e) => {
      e.preventDefault();
      action(element.dataset.id, "click");
    });
  } else if (node.role === "Image") {
    element.setAttribute("role", "img");
  } else if (node.role === "List") {
    element.setAttribute("role", "list");
  }
  return element;
}

/** A task's checkbox is named by the text next to it. */
function labelCheckbox(node, children) {
  if (node.role !== "ListItem") return;
  const checkbox = children.find((child) => child.getAttribute("role") === "checkbox");
  const text = children.find((child) => child !== checkbox);
  if (!checkbox || !text) return;
  text.id ||= `rs-editor-a11y-${text.dataset.id}`;
  checkbox.setAttribute("aria-labelledby", text.id);
}

/** The bounding box of `box` ([x0, y0, x1, y1]) under affine `t`. */
function transformBox(t, box) {
  if (!t) return box;
  const [a, b, c, d, e, f] = t;
  const xs = [], ys = [];
  for (const [x, y] of [[box[0], box[1]], [box[2], box[1]], [box[0], box[3]], [box[2], box[3]]]) {
    xs.push(a * x + c * y + e);
    ys.push(b * x + d * y + f);
  }
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/** The text between the selection's ends, in document order of the runs. */
function selectedText(runs, selection) {
  const ids = [...runs.keys()];
  let [a, ai] = selection.anchor;
  let [f, fi] = selection.focus;
  let start = ids.indexOf(a), end = ids.indexOf(f);
  if (start > end || (start === end && ai > fi)) {
    [start, end] = [end, start];
    [ai, fi] = [fi, ai];
  }
  if (start < 0 || end < 0) return "";
  let out = "";
  for (let i = start; i <= end; i++) {
    const characters = graphemes(runs.get(ids[i]).text ?? "");
    out += characters.slice(i === start ? ai : 0, i === end ? fi : characters.length).join("");
  }
  return out.replace(/\n/g, " ").trim();
}

/** Replaces `parent`'s children with `children`, moving rather than recreating. */
function replaceChildren(parent, children) {
  let current = parent.firstChild;
  for (const child of children) {
    if (current === child) {
      current = current.nextSibling;
    } else {
      parent.insertBefore(child, current);
    }
  }
  while (current) {
    const next = current.nextSibling;
    current.remove();
    current = next;
  }
}
