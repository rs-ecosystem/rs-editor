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
    this.elements = new Map();
    this.tree = null;
    this.runs = new Map();

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

  /** Rebuilds the mirror soon (at most once per idle period). */
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

  update() {
    const editor = this.editor;
    if (editor._destroyed) return;
    const tree = editor._json(editor._M._rse_accessibility_tree(editor._app));
    this.tree = tree;
    this.runs.clear();
    const seen = new Set();
    const children = (tree.children ?? []).map((child) => this.build(child, tree.bounds ?? [0, 0, 0, 0], seen));
    replaceChildren(this.root, children);
    for (const [id, element] of this.elements) {
      if (!seen.has(id)) this.elements.delete(id);
    }
  }

  /** The element for `node`, reusing the one with the same id. */
  build(node, parentBounds, seen) {
    seen.add(node.id);
    const tag = node.role === "Heading" ? `h${Math.min(6, Math.max(1, node.level ?? 2))}` : ROLE_ELEMENTS[node.role] ?? (node.role === "TextRun" ? "span" : "div");
    let element = this.elements.get(node.id);
    if (!element || element.localName !== tag) {
      element = document.createElement(tag);
      this.elements.set(node.id, element);
      if (node.role === "CheckBox") {
        element.setAttribute("role", "checkbox");
        element.tabIndex = -1;
        // Screen readers activate it with a click, which reaches the element
        // directly even though the mirror ignores the pointer.
        element.addEventListener("click", (e) => {
          e.preventDefault();
          this.action(element.dataset.id, "click");
        });
      } else if (node.role === "Image") {
        element.setAttribute("role", "img");
      } else if (node.role === "List") {
        element.setAttribute("role", "list");
      }
    }
    element.dataset.id = node.id;
    const bounds = node.bounds ?? parentBounds;
    const style = element.style;
    style.position = "absolute";
    style.margin = "0";
    style.padding = "0";
    style.left = `${bounds[0] - parentBounds[0]}px`;
    style.top = `${bounds[1] - parentBounds[1]}px`;
    style.width = `${bounds[2]}px`;
    style.height = `${bounds[3]}px`;
    if (node.role === "CheckBox") element.setAttribute("aria-checked", String(node.checked === true));
    if (node.role === "Image") element.setAttribute("aria-label", node.label ?? "");
    if (node.role === "TextRun") {
      this.runs.set(node.id, node);
      const text = (node.text ?? "").replace(/\n$/, "");
      if (element.textContent !== text) element.textContent = text;
      style.whiteSpace = "pre";
      style.overflow = "hidden";
      style.fontSize = `${Math.max(1, bounds[3] * 0.75)}px`;
      style.lineHeight = `${bounds[3]}px`;
    } else {
      replaceChildren(element, (node.children ?? []).map((child) => this.build(child, bounds, seen)));
    }
    return element;
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
    const tree = this.editor._json(this.editor._M._rse_accessibility_tree(this.editor._app));
    this.tree = tree;
    const runs = new Map();
    collectRuns(tree, runs);
    const selection = tree.selection;
    if (!selection) return;
    const after = this.editor.selection;
    const text = labels();
    if (event.shiftKey && !after.empty) {
      const selected = selectedText(runs, selection);
      this.announce(selected.length <= 80 ? `${selected} ${text.selected}` : `${selected.length} ${text.characters}`);
      return;
    }
    const [runId, index] = selection.focus;
    const run = runs.get(runId);
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

function collectRuns(node, runs) {
  if (node.role === "TextRun") runs.set(node.id, node);
  for (const child of node.children ?? []) collectRuns(child, runs);
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
  let index = 0;
  for (const child of children) {
    if (parent.childNodes[index] !== child) parent.insertBefore(child, parent.childNodes[index] ?? null);
    index++;
  }
  while (parent.childNodes.length > children.length) parent.lastChild.remove();
}
