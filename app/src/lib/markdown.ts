import { marked, type Token } from 'marked';

/** Block tokens for `text`. `tags`: text after an opening fence may be a language tag; Slack has none. */
export function mdTokens(text: string, tags = true): Token[] {
  const src = tags ? text : text.replace(/```/g, '\n```\n');
  return marked.lexer(src, { gfm: true, breaks: true });
}

export function safeHref(href: string): string | null {
  return /^https?:\/\//i.test(href) ? href : null;
}
