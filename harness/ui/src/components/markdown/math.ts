import type { TokenizerAndRendererExtension, Tokens } from "marked";

/**
 * Four math delimiter pairs added to marked's tokenizer, since markdown has no math of its own.
 * All four are accepted, because which pair a model emits depends on the provider.
 * It stops at tokens and never builds HTML, so model text never becomes markup.
 */

/** An empty TeX body is not a formula, just two dollar signs typed together. */
function token(
  type: string,
  raw: string | undefined,
  tex: string | undefined,
): Tokens.Generic | undefined {
  // Regex groups are `string | undefined` here, so filter once rather than spreading `!` at six call sites.
  const text = tex?.trim() ?? "";
  return raw === undefined || text === "" ? undefined : { type, raw, text };
}

/** A display formula, block-level: inline, marked would wrap it into the preceding paragraph. */
export const mathBlock: TokenizerAndRendererExtension = {
  name: "mathBlock",
  level: "block",

  // `start` points marked at the nearest possible formula, or the paragraph before swallows it.
  start(src: string) {
    return src.match(/\$\$|\\\[/)?.index;
  },

  tokenizer(src: string) {
    const dollars = /^\$\$([\s\S]+?)\$\$(?:\n+|$)/.exec(src);
    if (dollars !== null) return token("mathBlock", dollars[0], dollars[1]);
    const brackets = /^\\\[([\s\S]+?)\\\](?:\n+|$)/.exec(src);
    if (brackets !== null) return token("mathBlock", brackets[0], brackets[1]);
    return undefined;
  },

  // Unreachable today, since everything goes through `lexer()`; kept so `parse()` would emit text, not markup.
  renderer(t) {
    return t.text;
  },
};

/** An inline formula; the hard case is currency, so no space inside the delimiters, no digit after the closer, and no empty body, checked in code rather than by lookbehind. */
export const mathInline: TokenizerAndRendererExtension = {
  name: "mathInline",
  level: "inline",

  start(src: string) {
    return src.match(/\$|\\\(/)?.index;
  },

  tokenizer(src: string) {
    const parens = /^\\\(([\s\S]+?)\\\)/.exec(src);
    if (parens !== null) return token("mathInline", parens[0], parens[1]);

    const dollars = /^\$((?:\\.|[^\\$])+?)\$(?!\d)/.exec(src);
    const body = dollars?.[1];
    if (body === undefined) return undefined;
    if (/^\s/.test(body) || /\s$/.test(body)) return undefined;
    return token("mathInline", dollars?.[0], body);
  },

  renderer(t) {
    return t.text;
  },
};

export const MATH_EXTENSIONS = [mathBlock, mathInline];
