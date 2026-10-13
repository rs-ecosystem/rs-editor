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
  /** Tiptap JSON, or an HTML string. Not an undoable change. */
  content?: JSONContent | string;
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
  readonly isEmpty: boolean;
  /** Tiptap JSON or HTML; false when the content does not fit the schema. */
  setContent(content: JSONContent | string): boolean;

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
