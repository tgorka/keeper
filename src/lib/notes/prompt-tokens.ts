/** UTF-16 spans deliberately match textarea selection offsets; whitespace is never rewritten. */
export interface PromptToken {
  start: number;
  end: number;
  text: string;
}

export function tokenize(text: string): PromptToken[] {
  return Array.from(text.matchAll(/\S+/gu), (match) => ({
    start: match.index,
    end: match.index + match[0].length,
    text: match[0],
  }));
}

export function tokenAtCaret(text: string, caret: number): PromptToken | null {
  return tokenize(text).find((span) => span.start <= caret && caret <= span.end) ?? null;
}

export function replaceSpan(text: string, span: PromptToken, replacement = "") {
  return {
    text: text.slice(0, span.start) + replacement + text.slice(span.end),
    caret: span.start + replacement.length,
  };
}
