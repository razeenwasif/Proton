/* The part of KaTeX that ui/markdown.js uses. The library itself is not type
 * checked: it is someone else's, vendored as published. */

export interface KatexOptions {
  displayMode?: boolean;
  output?: "html" | "mathml" | "htmlAndMathml";
  throwOnError?: boolean;
  errorColor?: string;
  strict?: boolean | "ignore" | "warn" | "error";
  trust?: boolean;
  maxSize?: number;
  maxExpand?: number;
}

declare const katex: {
  renderToString(tex: string, options?: KatexOptions): string;
  render(tex: string, element: HTMLElement, options?: KatexOptions): void;
  version: string;
};

export default katex;
