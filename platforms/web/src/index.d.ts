/** A font file to load when the text needs one of its characters. */
export interface FontSource {
  family: string;
  /** A TTF, OTF or WOFF file (not WOFF2). */
  url?: string;
  /**
   * A font split by Unicode range as fontsource packages are
   * (`<base>unicode.json`, `<base>files/<id>-<subset>-<weight>-normal.woff`):
   * only the subsets with characters the text uses are loaded.
   */
  subsets?: { base: string; id: string; weight?: number };
  /** Code point ranges `[first, last]` the font covers; any character when omitted. */
  ranges?: [number, number][];
}

/** Noto emoji and CJK fonts on public CDNs (large; self-host for production). */
export const notoFonts: {
  emoji: FontSource;
  japanese: FontSource;
  korean: FontSource;
  simplifiedChinese: FontSource;
};

/** Tiptap JSON: a document node such as `{ type: "doc", content: [...] }`. */
export type JSONContent = {
  type?: string;
  attrs?: Record<string, unknown>;
  content?: JSONContent[];
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  text?: string;
  [key: string]: unknown;
};

export interface EditorOptions {
  /** Element the editor fills (it gets `position: relative` if static). */
  element: HTMLElement;
  /** Tiptap JSON, or an HTML (or Markdown) string. Not an undoable change. */
  content?: JSONContent | string;
  /** How to read a string `content`: HTML by default. */
  contentType?: "html" | "markdown";
  /** Read pasted plain text as Markdown (off by default). */
  pasteMarkdown?: boolean;
  /**
   * A formatting menu (bold, italic, underline, strike, code, highlight, link) over
   * text selected with a mouse or trackpad, drawn in the canvas (on by
   * default; touch selections get the selection menu instead).
   */
  bubbleMenu?: boolean;
  /** Shows a menu of blocks (headings, lists, quote, code…) when `/` is typed. Default true. */
  slashMenu?: boolean;
  /** A grip beside the block under the mouse (or with the caret, on touch screens) to select and drag blocks. Default true. */
  dragHandle?: boolean;
  /** Text in an empty line with the caret. Default: a hint at the `/` menu in the UI's language. */
  placeholder?: string;
  /** The most characters the document may hold (Tiptap's CharacterCount `limit`). */
  characterLimit?: number;
  /**
   * Stores an image file (picked from the `/` menu, pasted or dropped) and
   * resolves to its address, for example after uploading it to your server.
   * Without it, the `/` menu asks for an image address instead.
   */
  uploadImage?: (file: File) => Promise<string>;
  /**
   * Loads the picture of an image node (`src` is its address). The editor
   * reads image pixels, which browsers allow only for images from this
   * site or from servers sending CORS headers; for others, fetch through
   * your server and resolve to a `Blob` or a same-origin (or CORS) URL.
   * Resolve to `undefined` to load `src` as usual, or to `null` when it
   * cannot be loaded (it shows as broken).
   */
  loadImage?: (src: string) => Promise<Blob | string | null | undefined>;
  /** Language of the editor's own menus, as a BCP 47 tag. Default `navigator.language`; English when there is no translation. */
  locale?: string;
  /**
   * Fonts loaded on demand when the text has characters the bundled fonts
   * (Latin, Arabic, Devanagari) lack, such as emoji or CJK. See `notoFonts`.
   */
  fonts?: FontSource[];
  /**
   * Mirror the document into invisible DOM for screen readers and announce
   * caret moves (on by default).
   */
  accessibility?: boolean;
  dark?: boolean;
  editable?: boolean;
  /** URL of `rs-editor-web.js` (default: `../dist/` next to this module). */
  wasmUrl?: string;
  /** Force the hidden textarea instead of EditContext. */
  input?: "textarea";
}

/**
 * Commands to run together as one transaction (one undo step). Any command
 * of the editor's extensions works as a method, with its Tiptap arguments.
 */
export interface ChainedCommands {
  /** Focuses the editor (and shows the on-screen keyboard) after running. */
  focus(): ChainedCommands;
  /** Any command by name. */
  command(name: string, args?: unknown): ChainedCommands;
  /** Runs the chain, or for `can()` checks it; true only if every command applies. */
  run(): boolean;

  toggleBold(): ChainedCommands;
  toggleItalic(): ChainedCommands;
  toggleUnderline(): ChainedCommands;
  toggleStrike(): ChainedCommands;
  toggleCode(): ChainedCommands;
  toggleHighlight(attrs?: { color?: string }): ChainedCommands;
  toggleSubscript(): ChainedCommands;
  toggleSuperscript(): ChainedCommands;
  unsetAllMarks(): ChainedCommands;
  setParagraph(): ChainedCommands;
  toggleHeading(attrs: { level: 1 | 2 | 3 | 4 | 5 | 6 }): ChainedCommands;
  toggleBulletList(): ChainedCommands;
  toggleOrderedList(): ChainedCommands;
  toggleTaskList(): ChainedCommands;
  toggleBlockquote(): ChainedCommands;
  toggleCodeBlock(): ChainedCommands;
  setHorizontalRule(): ChainedCommands;
  setHardBreak(): ChainedCommands;
  sinkListItem(): ChainedCommands;
  liftListItem(): ChainedCommands;
  setLink(attrs: { href: string; target?: string | null }): ChainedCommands;
  unsetLink(): ChainedCommands;
  setTextAlign(alignment: "left" | "center" | "right" | "justify"): ChainedCommands;
  unsetTextAlign(): ChainedCommands;
  selectAll(): ChainedCommands;
  undo(): ChainedCommands;
  redo(): ChainedCommands;
  [command: string]: (args?: any) => ChainedCommands;
}

export type SingleCommands = {
  [K in keyof ChainedCommands]: ChainedCommands[K] extends (...args: infer A) => ChainedCommands
    ? (...args: A) => boolean
    : never;
};

export interface EditorEvents {
  /** Any change to the document or the selection. */
  transaction: { editor: Editor };
  /** The document changed. */
  update: { editor: Editor };
  /** Only the selection changed. */
  selectionUpdate: { editor: Editor };
  /** An edit was refused (or a paste cut short) at the `characterLimit`. */
  characterLimit: { editor: Editor };
  /** `uploadImage` failed: the image it was storing is removed again. */
  uploadError: { editor: Editor; file: File; error: unknown };
  focus: {};
  blur: {};
}

export interface Editor {
  readonly element: HTMLElement;
  readonly canvas: HTMLCanvasElement;
  /** How text input reaches the editor. */
  readonly inputMode: "EditContext" | "textarea";
  readonly stats: { frames: number; lastFrameMs: number };
  /** `editor.commands.toggleBold()`: one command; false when it does not apply. */
  readonly commands: SingleCommands;

  chain(): ChainedCommands;
  /** `editor.can().toggleBold().run()` checks without changing anything. */
  can(): ChainedCommands;
  isActive(name: string, attrs?: Record<string, unknown>): boolean;
  getAttributes(name: string): Record<string, unknown>;

  getJSON(): JSONContent;
  getHTML(): string;
  getText(): string;
  /** GitHub-flavored Markdown; marks Markdown lacks are written as HTML tags. */
  getMarkdown(): string;
  readonly isEmpty: boolean;
  /**
   * Tiptap JSON, HTML, or Markdown with `{ contentType: "markdown" }`; false
   * when the content does not fit the schema.
   */
  setContent(content: JSONContent | string, options?: { contentType?: "html" | "markdown" }): boolean;
  setPasteMarkdown(markdown: boolean): void;
  /** Opens the link address field over the link at the caret or the selected text, as Mod-K does. False when there is nothing to link. */
  editLink(): boolean;
  /** Text in an empty line with the caret; "" restores the default. */
  setPlaceholder(text: string): void;
  /** Tiptap's storage: `storage.characterCount.characters()`, `.words()` and `.limit()`. */
  readonly storage: {
    characterCount: { characters(): number; words(): number; limit(): number | null };
  };
  /** Language of the editor's own menus, such as "id-ID". */
  setLocale(locale: string): void;
  /** Adds a font (TTF/OTF/WOFF bytes); returns its family, or "" when the bytes are not a font. */
  addFont(bytes: ArrayBuffer | Uint8Array, family?: string): string;

  readonly isEditable: boolean;
  setEditable(editable: boolean): void;
  setDark(dark: boolean): void;
  focus(): void;

  on<E extends keyof EditorEvents>(event: E, callback: (payload: EditorEvents[E]) => void): Editor;
  off<E extends keyof EditorEvents>(event: E, callback: (payload: EditorEvents[E]) => void): Editor;

  /** Frees the editor and removes its elements. */
  destroy(): void;
}

/** Loads the WebAssembly module (once) and creates an editor in `options.element`. */
export function createEditor(options: EditorOptions): Promise<Editor>;
