/* Code in a reply, coloured the way the editor colours code.
 *
 * The editor's own colouring is tree-sitter, in the core, over a buffer. A
 * fenced block in a reply is a few lines that arrive a word at a time, so it
 * is coloured here instead, by a small lexer for each family of languages.
 * A lexer and not a parser: comments, strings, numbers, keywords, types,
 * calls, operators and punctuation, drawn with the classes the editor's own
 * runs use — `.k .fn .str .num .cm .ty .op .pn`, and `.cn .pr .at` for
 * constants, properties and attributes — so a block in a reply wears the
 * theme the code beside it does. Diffs add two of their own, `.da` and `.dd`.
 *
 * Every character of the source ends up inside text this file escaped; the
 * only markup is the spans it writes. */

import { escapeHtml } from "./html.js";

/** @param {string} list */
const words = (list) => new Set(list.split(/\s+/).filter(Boolean));

/**
 * @typedef {{
 *   line?: string[], block?: [string, string][], quotes?: string[],
 *   triple?: boolean, prefixes?: string,
 *   keywords?: Set<string>, types?: Set<string>, constants?: Set<string>,
 *   typeCase?: boolean, macros?: boolean, attributes?: boolean, lifetimes?: boolean,
 *   rawStrings?: boolean, decorators?: boolean, preprocessor?: boolean,
 *   shell?: boolean, caseInsensitive?: boolean, variables?: boolean
 * }} Lang
 */

/** @type {{line: string[], block: [string, string][]}} */
const C_COMMENTS = { line: ["//"], block: [["/*", "*/"]] };

const JS_KEYWORDS = "async await break case catch class const continue debugger default delete do else export extends " +
  "finally for from function get if import in instanceof let new of return set static super switch this throw try typeof " +
  "var void while with yield";

/** @type {Record<string, Lang>} */
const LANGS = {
  rust: {
    ...C_COMMENTS, quotes: ['"'], macros: true, attributes: true, lifetimes: true, rawStrings: true, typeCase: true,
    keywords: words("as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod " +
      "move mut pub ref return self Self static struct super trait type union unsafe use where while yield"),
    types: words("bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64"),
    constants: words("true false None Some Ok Err"),
  },
  python: {
    line: ["#"], quotes: ['"', "'"], triple: true, prefixes: "rRbBfFuU", decorators: true, typeCase: true,
    keywords: words("and as assert async await break class continue def del elif else except finally for from global if " +
      "import in is lambda nonlocal not or pass raise return try while with yield match case"),
    types: words("int float complex str bytes bool list dict set frozenset tuple object type"),
    constants: words("True False None self cls"),
  },
  javascript: {
    ...C_COMMENTS, quotes: ['"', "'", "`"], typeCase: true, decorators: true,
    keywords: words(JS_KEYWORDS),
    constants: words("true false null undefined NaN Infinity"),
  },
  typescript: {
    ...C_COMMENTS, quotes: ['"', "'", "`"], typeCase: true, decorators: true,
    keywords: words(`${JS_KEYWORDS} type interface enum declare namespace abstract implements private protected public ` +
      "readonly as is keyof infer satisfies"),
    types: words("string number boolean void object symbol bigint any unknown never"),
    constants: words("true false null undefined NaN Infinity"),
  },
  c: {
    ...C_COMMENTS, quotes: ['"', "'"], preprocessor: true,
    keywords: words("auto break case const continue default do else enum extern for goto if inline register restrict return " +
      "sizeof static struct switch typedef union volatile while _Alignas _Alignof _Atomic _Generic _Noreturn _Static_assert"),
    types: words("char double float int long short signed unsigned void bool _Bool size_t ssize_t ptrdiff_t intptr_t uintptr_t " +
      "int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t FILE"),
    constants: words("NULL true false"),
  },
  cpp: {
    ...C_COMMENTS, quotes: ['"', "'"], preprocessor: true, typeCase: true,
    keywords: words("alignas alignof and asm auto break case catch class concept const consteval constexpr constinit const_cast " +
      "continue co_await co_return co_yield decltype default delete do dynamic_cast else enum explicit export extern final " +
      "for friend goto if inline mutable namespace new noexcept not operator or override private protected public " +
      "reinterpret_cast requires return sizeof static static_assert static_cast struct switch template this thread_local " +
      "throw try typedef typeid typename union using virtual volatile while"),
    types: words("bool char char8_t char16_t char32_t double float int long short signed unsigned void wchar_t size_t " +
      "int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t std string vector map set unordered_map " +
      "unique_ptr shared_ptr optional"),
    constants: words("true false nullptr NULL"),
  },
  go: {
    ...C_COMMENTS, quotes: ['"', "'", "`"],
    keywords: words("break case chan const continue default defer else fallthrough for func go goto if import interface map " +
      "package range return select struct switch type var"),
    types: words("bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 " +
      "uint16 uint32 uint64 uintptr any"),
    constants: words("true false nil iota"),
  },
  java: {
    ...C_COMMENTS, quotes: ['"', "'"], decorators: true, typeCase: true,
    keywords: words("abstract assert break case catch class const continue default do else enum extends final finally for " +
      "goto if implements import instanceof interface native new package private protected public record return sealed " +
      "permits static strictfp super switch synchronized this throw throws transient try var void volatile while yield"),
    types: words("boolean byte char double float int long short"),
    constants: words("true false null"),
  },
  kotlin: {
    ...C_COMMENTS, quotes: ['"', "'"], triple: true, decorators: true, typeCase: true,
    keywords: words("as break class continue do else for fun if in interface is object package return super this throw " +
      "try typealias typeof val var when while by catch constructor data enum finally get import init inner internal " +
      "lateinit open override private protected public sealed set suspend"),
    constants: words("true false null"),
  },
  swift: {
    ...C_COMMENTS, quotes: ['"'], triple: true, decorators: true, typeCase: true,
    keywords: words("associatedtype class deinit enum extension fileprivate func import init inout internal let open " +
      "operator private protocol public rethrows static struct subscript typealias var break case continue default defer " +
      "do else fallthrough for guard if in repeat return switch where while as catch is super self Self throw throws try " +
      "async await some any"),
    constants: words("true false nil"),
  },
  csharp: {
    ...C_COMMENTS, quotes: ['"', "'"], typeCase: true, preprocessor: true,
    keywords: words("abstract as base break case catch checked class const continue default delegate do else enum event " +
      "explicit extern finally fixed for foreach goto if implicit in interface internal is lock namespace new operator out " +
      "override params private protected public readonly ref return sealed sizeof stackalloc static struct switch this " +
      "throw try typeof unchecked unsafe using virtual void volatile while async await var record init get set yield"),
    types: words("bool byte char decimal double float int long object sbyte short string uint ulong ushort"),
    constants: words("true false null"),
  },
  ruby: {
    line: ["#"], quotes: ['"', "'"], typeCase: true,
    keywords: words("alias and begin break case class def defined? do else elsif end ensure for if in module next not or " +
      "redo rescue retry return self super then undef unless until when while yield require attr_accessor attr_reader"),
    constants: words("true false nil"),
  },
  php: {
    line: ["//", "#"], block: [["/*", "*/"]], quotes: ['"', "'"], variables: true,
    keywords: words("abstract and array as break callable case catch class clone const continue declare default do echo " +
      "else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function " +
      "global goto if implements include instanceof insteadof interface isset list match namespace new or print private " +
      "protected public readonly require return static switch throw trait try unset use var while yield"),
    constants: words("true false null TRUE FALSE NULL"),
  },
  lua: {
    line: ["--"], block: [["--[[", "]]"]], quotes: ['"', "'"],
    keywords: words("and break do else elseif end for function goto if in local not or repeat return then until while"),
    constants: words("true false nil"),
  },
  r: {
    line: ["#"], quotes: ['"', "'"],
    keywords: words("if else repeat while function for in next break return library require"),
    constants: words("TRUE FALSE NULL NA NaN Inf T F"),
  },
  julia: {
    line: ["#"], block: [["#=", "=#"]], quotes: ['"'], triple: true, typeCase: true,
    keywords: words("baremodule begin break catch const continue do else elseif end export finally for function global if " +
      "import let local macro module quote return struct try using while abstract mutable primitive type where in"),
    constants: words("true false nothing missing NaN Inf"),
  },
  haskell: {
    line: ["--"], block: [["{-", "-}"]], quotes: ['"'], typeCase: true,
    keywords: words("case class data default deriving do else foreign if import in infix infixl infixr instance let module " +
      "newtype of then type where"),
    constants: words("True False Nothing Just"),
  },
  zig: {
    line: ["//"], quotes: ['"', "'"], typeCase: true,
    keywords: words("addrspace align allowzero and anyframe anytype asm async await break callconv catch comptime const " +
      "continue defer else enum errdefer error export extern fn for if inline linksection noalias noinline nosuspend opaque " +
      "or orelse packed pub resume return struct suspend switch test threadlocal try union unreachable usingnamespace var " +
      "volatile while"),
    types: words("i8 u8 i16 u16 i32 u32 i64 u64 i128 u128 isize usize f16 f32 f64 f128 bool void noreturn type anyerror"),
    constants: words("true false null undefined"),
  },
  dart: {
    ...C_COMMENTS, quotes: ['"', "'"], triple: true, decorators: true, typeCase: true,
    keywords: words("abstract as assert async await break case catch class const continue covariant default deferred do " +
      "dynamic else enum export extends extension external factory final finally for get hide if implements import in " +
      "interface is late library mixin new on operator part required rethrow return set show static super switch sync this " +
      "throw try typedef var void while with yield"),
    constants: words("true false null"),
  },
  scala: {
    ...C_COMMENTS, quotes: ['"', "'"], triple: true, decorators: true, typeCase: true,
    keywords: words("abstract case catch class def do else enum export extends final finally for given if implicit import " +
      "lazy match new object override package private protected return sealed super then this throw trait try type using " +
      "val var while with yield"),
    constants: words("true false null"),
  },
  shell: {
    line: ["#"], quotes: ['"', "'"], shell: true, variables: true,
    keywords: words("if then else elif fi for while until do done case esac in function return select time local export " +
      "readonly declare unset shift exit break continue source alias set"),
  },
  sql: {
    line: ["--"], block: [["/*", "*/"]], quotes: ["'", '"'], caseInsensitive: true,
    keywords: words("select from where and or not insert into values update set delete create table view index drop alter " +
      "add column primary key foreign references join inner left right full outer cross on as group by order having limit " +
      "offset distinct union all case when then else end exists in is null like between returning with recursive asc desc " +
      "begin commit rollback transaction default unique check constraint if replace cascade"),
    types: words("int integer bigint smallint serial bigserial real double precision float numeric decimal text varchar " +
      "char boolean bool date time timestamp timestamptz interval json jsonb uuid blob bytea"),
    constants: words("true false"),
  },
  latex: { line: ["%"] },
  lean: {
    line: ["--"], block: [["/-", "-/"]], quotes: ['"'], typeCase: true,
    keywords: words("theorem lemma def example abbrev instance structure class inductive where with fun by have show from at " +
      "calc match let in if then else do return namespace section end open variable universe import set_option attribute " +
      "local private protected noncomputable deriving extends mutual termination_by decreasing_by sorry " +
      "intro intros apply exact refine rfl simp simp_all rw rwa ring ring_nf linarith nlinarith omega norm_num decide " +
      "induction cases rcases obtain use constructor exists aesop positivity field_simp push_neg by_contra contradiction " +
      "exfalso trivial gcongr unfold specialize"),
    types: words("Prop Type Sort ℕ ℤ ℚ ℝ ℂ Nat Int Rat Real Complex Bool List Finset Set"),
    constants: words("True False true false"),
  },
};

/** Languages with a lexer of their own rather than the shared one. */
const SPECIAL = new Set(["json", "yaml", "toml", "html", "xml", "css", "diff"]);

/** What a fence's language may also be called. */
const ALIASES = /** @type {Record<string, string>} */ ({
  rs: "rust", py: "python", python3: "python", py3: "python", js: "javascript", jsx: "javascript", mjs: "javascript",
  cjs: "javascript", node: "javascript", ts: "typescript", tsx: "typescript", mts: "typescript", h: "c", "c++": "cpp",
  cc: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", golang: "go", kt: "kotlin", kts: "kotlin", cs: "csharp", "c#": "csharp",
  rb: "ruby", sh: "shell", bash: "shell", zsh: "shell", fish: "shell", console: "shell", "shell-session": "shell",
  shellsession: "shell", ksh: "shell", ps1: "shell", powershell: "shell", jl: "julia", hs: "haskell", tex: "latex",
  yml: "yaml", htm: "html", xhtml: "html", svg: "xml", plist: "xml", vue: "html", scss: "css", less: "css", sass: "css",
  patch: "diff", jsonc: "json", json5: "json", geojson: "json", ini: "toml", conf: "toml", cfg: "toml", properties: "toml",
  dockerfile: "shell", docker: "shell", makefile: "shell", make: "shell", cmake: "shell", psql: "sql", mysql: "sql",
  sqlite: "sql", postgres: "sql", postgresql: "sql", glsl: "c", hlsl: "c", wgsl: "rust", metal: "cpp", cu: "cpp",
  cuda: "cpp", objc: "c", "objective-c": "c", proto: "c", protobuf: "c", gradle: "kotlin", groovy: "java",
  sol: "javascript", solidity: "javascript", lean4: "lean",
});

/** The language a fence names, as one this file knows, or "".
 *  @param {string} lang */
export function language(lang) {
  const name = String(lang ?? "").trim().toLowerCase();
  const known = ALIASES[name] ?? name;
  return known in LANGS || SPECIAL.has(known) ? known : "";
}

/** A run of source in a class. @param {string} cls @param {string} text */
const span = (cls, text) => (text ? `<span class="${cls}">${escapeHtml(text)}</span>` : "");

const IDENT = /[A-Za-z_$À-￿][\w$À-￿]*\??/y;
// `1..10` is two numbers and a range, and `1.max(2)` a method on one.
const NUMBER = /(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*(?:\.\d[\d_]*|\.(?![.\w]))?|\.\d[\d_]*)(?:[eE][+-]?\d[\d_]*)?)(?:[A-Za-z_]\w*)?/y;
const OPERATOR = /[+\-*/%=<>!&|^~?:]+/y;
const PUNCT = /[{}()[\];,.@#\\]/;
const CAMEL = /^[A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*$/;
const SHOUTED = /^[A-Z][A-Z0-9_]+$/;
/** The shell words a command follows: `then ls`, but not `for ls`. */
const LEADS_TO_COMMAND = words("if then do else elif while until time");

/** Whether `c` can be part of a name. @param {string | undefined} c */
const namelike = (c) => c !== undefined && /[\w$À-￿]/.test(c);

/** Where a string that opens at `start` with `quote` ends. A backslash
 *  escapes the next character; a plain string ends at its line.
 *  @param {string} code @param {number} start @param {string} quote @param {boolean} multiline */
function stringEnd(code, start, quote, multiline) {
  let i = start + quote.length;
  while (i < code.length) {
    const c = code[i];
    if (c === "\\") {
      i += 2;
      continue;
    }
    if (code.startsWith(quote, i)) return i + quote.length;
    if (c === "\n" && !multiline) return i;
    i++;
  }
  return code.length;
}

/** What a sticky pattern matches at `start`, or "".
 *  @param {RegExp} pattern @param {string} code @param {number} start */
function matchAt(pattern, code, start) {
  pattern.lastIndex = start;
  return pattern.exec(code)?.[0] ?? "";
}

/** The shared lexer, for the C, scripting and ML families.
 *  @param {string} code @param {Lang} L */
function lexGeneric(code, L) {
  let out = "";
  let plain = "";
  /** @param {string} cls @param {string} text */
  const token = (cls, text) => {
    if (plain) out += escapeHtml(plain);
    plain = "";
    out += span(cls, text);
  };
  const n = code.length;
  let i = 0;
  // Where a shell looks for a command: the start of a line, and after a
  // pipe, `&&`, `;`, `$(` or a keyword like `then`.
  let commandAt = true;
  while (i < n) {
    const c = code[i];
    if (c === "\n" || c === " " || c === "\t") {
      plain += c;
      if (c === "\n") commandAt = true;
      i++;
      continue;
    }
    const command = commandAt;
    commandAt = false;

    const block = L.block?.find(([open]) => code.startsWith(open, i));
    if (block) {
      const end = code.indexOf(block[1], i + block[0].length);
      const stop = end < 0 ? n : end + block[1].length;
      token("cm", code.slice(i, stop));
      i = stop;
      continue;
    }
    // In a shell a `#` only starts a comment at the start of a word.
    if (L.line?.some((open) => code.startsWith(open, i)) && (!L.shell || i === 0 || /\s/.test(code[i - 1]))) {
      const end = code.indexOf("\n", i);
      const stop = end < 0 ? n : end;
      token("cm", code.slice(i, stop));
      i = stop;
      continue;
    }
    // A prompt is not part of the command after it.
    if (L.shell && command && (c === "$" || c === "%" || c === ">") && code[i + 1] === " ") {
      token("pn", c);
      i++;
      commandAt = true;
      continue;
    }
    if (L.shell && code.startsWith("$(", i)) {
      token("pn", "$(");
      i += 2;
      commandAt = true;
      continue;
    }
    if (L.preprocessor && c === "#" && command) {
      const directive = matchAt(/#\s*\w+/y, code, i);
      if (directive) {
        token("k", directive);
        i += directive.length;
        continue;
      }
    }
    if (L.attributes && (code.startsWith("#[", i) || code.startsWith("#![", i))) {
      let depth = 0;
      let j = i;
      for (; j < n && code[j] !== "\n"; j++) {
        if (code[j] === "[") depth++;
        else if (code[j] === "]" && --depth === 0) {
          j++;
          break;
        }
      }
      token("at", code.slice(i, j));
      i = j;
      continue;
    }
    if (L.decorators && c === "@" && /[A-Za-z_]/.test(code[i + 1] ?? "")) {
      const decorator = matchAt(/@[\w.]+/y, code, i);
      token("at", decorator);
      i += decorator.length;
      continue;
    }
    if (L.rawStrings && (c === "r" || c === "b") && !namelike(code[i - 1])) {
      const open = /b?r(#*)"/y;
      open.lastIndex = i;
      const found = open.exec(code);
      if (found) {
        const close = `"${found[1]}`;
        const end = code.indexOf(close, i + found[0].length);
        const stop = end < 0 ? n : end + close.length;
        token("str", code.slice(i, stop));
        i = stop;
        continue;
      }
    }
    // A string's prefix — f"…", r'…', b"…" — is part of the string.
    const prefix = L.prefixes?.includes(c) && !namelike(code[i - 1]) ? matchAt(/[rRbBfFuU]{1,2}(?=["'])/y, code, i).length : 0;
    const q = code[i + prefix];
    if (L.triple && (code.startsWith('"""', i + prefix) || code.startsWith("'''", i + prefix))) {
      const stop = stringEnd(code, i + prefix, code.slice(i + prefix, i + prefix + 3), true);
      token("str", code.slice(i, stop));
      i = stop;
      continue;
    }
    if (L.lifetimes && c === "'") {
      const char = matchAt(/'(?:\\(?:u\{[\da-fA-F]+\}|x[\da-fA-F]{2}|.)|[^\\'\n])'/y, code, i);
      const lifetime = char ? "" : matchAt(/'[A-Za-z_]\w*/y, code, i);
      if (char || lifetime) {
        token(char ? "str" : "ty", char || lifetime);
        i += (char || lifetime).length;
        continue;
      }
    }
    if (q !== undefined && L.quotes?.includes(q)) {
      const stop = stringEnd(code, i + prefix, q, q === "`");
      token("str", code.slice(i, stop));
      i = stop;
      continue;
    }
    if (L.variables && c === "$") {
      const variable = matchAt(/\$(?:\{[^}\n]*\}|[A-Za-z_]\w*|[0-9@#?$!*-])/y, code, i);
      if (variable) {
        token("cn", variable);
        i += variable.length;
        continue;
      }
    }
    if ((/\d/.test(c) || (c === "." && /\d/.test(code[i + 1] ?? ""))) && !namelike(code[i - 1])) {
      const number = matchAt(NUMBER, code, i);
      if (number) {
        token("num", number);
        i += number.length;
        continue;
      }
    }
    if (namelike(c) && !/\d/.test(c)) {
      let word = matchAt(IDENT, code, i) || c;
      // A name ends in `?` only in Ruby, and only as one of its own.
      if (word.length > 1 && word.endsWith("?") && !L.keywords?.has(word)) word = word.slice(0, -1);
      const end = i + word.length;
      const key = L.caseInsensitive ? word.toLowerCase() : word;
      const next = code.slice(end, end + 32).match(/^[ \t]*(\S)/)?.[1];
      i = end;
      if (L.keywords?.has(key)) {
        token("k", word);
        if (L.shell && LEADS_TO_COMMAND.has(word)) commandAt = true;
      } else if (L.constants?.has(word)) token("cn", word);
      else if (L.macros && code[end] === "!" && code[end + 1] !== "=") {
        token("fn", `${word}!`);
        i = end + 1;
      } else if (L.shell && code[end] === "=") token("pr", word);
      else if (L.shell && command) token("fn", word);
      else if (L.types?.has(key) || (L.typeCase && CAMEL.test(word))) token("ty", word);
      else if (!L.shell && !L.caseInsensitive && SHOUTED.test(word)) token("cn", word);
      else if (next === "(") token("fn", word);
      else plain += word;
      continue;
    }
    if (L.shell && c === "-" && /\s/.test(code[i - 1] ?? " ") && /[A-Za-z-]/.test(code[i + 1] ?? "")) {
      const flag = matchAt(/--?[\w-]+/y, code, i);
      token("at", flag);
      i += flag.length;
      continue;
    }
    const op = matchAt(OPERATOR, code, i);
    if (op) {
      token("op", op);
      i += op.length;
      if (L.shell && /[|&]$/.test(op)) commandAt = true;
      continue;
    }
    if (PUNCT.test(c)) {
      token("pn", c);
      i++;
      if (L.shell && c === ";") commandAt = true;
      continue;
    }
    plain += c;
    i++;
  }
  if (plain) out += escapeHtml(plain);
  return out;
}

/** JSON: keys as properties, the rest as values. @param {string} code */
function lexJson(code) {
  return code.replace(
    /("(?:\\.|[^"\\\n])*"?)(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|(\/\/[^\n]*|\/\*[\s\S]*?(?:\*\/|$))|([{}[\],])|([^"\d/{}[\],tfn-]+|[\s\S])/g,
    (_, str, colon, num, literal, comment, punct, other) => {
      if (str !== undefined) {
        return colon ? span("pr", str) + escapeHtml(colon.slice(0, -1)) + span("pn", ":") : span("str", str);
      }
      if (num !== undefined) return span("num", num);
      if (literal !== undefined) return span("cn", literal);
      if (comment !== undefined) return span("cm", comment);
      if (punct !== undefined) return span("pn", punct);
      return escapeHtml(other);
    },
  );
}

/** YAML, a line at a time: keys, comments, scalars. @param {string} code */
function lexYaml(code) {
  /** @param {string} v */
  const value = (v) => {
    const trimmed = v.trim();
    if (!trimmed) return escapeHtml(v);
    const lead = escapeHtml(v.slice(0, v.length - v.trimStart().length));
    const trail = escapeHtml(v.slice(v.trimEnd().length));
    if (/^(["']).*\1$/.test(trimmed)) return lead + span("str", trimmed) + trail;
    if (/^[-+]?\d[\d_]*(\.\d+)?([eE][-+]?\d+)?$/.test(trimmed)) return lead + span("num", trimmed) + trail;
    if (/^(true|false|null|yes|no|on|off|~)$/i.test(trimmed)) return lead + span("cn", trimmed) + trail;
    if (/^[|>][-+]?$/.test(trimmed) || /^[&*!]\S*$/.test(trimmed)) return lead + span("op", trimmed) + trail;
    return escapeHtml(v);
  };
  return code.split("\n").map((line) => {
    const comment = line.match(/(^|\s)#.*$/);
    const cut = comment ? (comment.index ?? 0) + comment[1].length : line.length;
    const body = line.slice(0, cut);
    const tail = span("cm", line.slice(cut));
    if (/^\s*(---|\.\.\.)\s*$/.test(body)) return span("pn", body) + tail;
    const key = body.match(/^(\s*)(-\s+)?("[^"]*"|'[^']*'|[^\s:#'"][^:#]*?)(\s*:)(?=\s|$)(.*)$/);
    if (key) {
      const dash = key[2] ? span("pn", "-") + escapeHtml(key[2].slice(1)) : "";
      return escapeHtml(key[1]) + dash + span("pr", key[3]) + span("pn", key[4]) + value(key[5]) + tail;
    }
    const item = body.match(/^(\s*)(-)(\s.*|)$/);
    if (item) return escapeHtml(item[1]) + span("pn", item[2]) + value(item[3]) + tail;
    return value(body) + tail;
  }).join("\n");
}

/** TOML and INI: sections, keys and values. @param {string} code */
function lexToml(code) {
  /** @type {Lang} */
  const values = { line: ["#"], quotes: ['"', "'"], constants: words("true false") };
  return code.split("\n").map((line) => {
    if (/^\s*[#;]/.test(line)) return span("cm", line);
    const section = line.match(/^(\s*)(\[\[?[^\]]*\]\]?)(.*)$/);
    if (section) return escapeHtml(section[1]) + span("ty", section[2]) + lexGeneric(section[3], values);
    const pair = line.match(/^(\s*)([\w.\-"' ]+?)(\s*=\s*)(.*)$/);
    if (!pair) return lexGeneric(line, values);
    return escapeHtml(pair[1]) + span("pr", pair[2]) + span("op", pair[3]) + lexGeneric(pair[4], values);
  }).join("\n");
}

/** HTML and XML: tags, attributes and their values, comments, entities.
 *  @param {string} code */
function lexMarkup(code) {
  return code.replace(
    /(<!--[\s\S]*?(?:-->|$))|(<!\[CDATA\[[\s\S]*?(?:\]\]>|$))|(<\/?)([\w:.-]+)((?:\s+[\w:.@-]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)(\s*\/?>)?|(<[!?][^>]*>?)|([^<]+|<)/g,
    (whole, comment, cdata, open, name, attrs, close, declaration, text) => {
      if (comment !== undefined) return span("cm", comment);
      if (cdata !== undefined) return span("str", cdata);
      if (declaration !== undefined) return span("cm", declaration);
      if (text !== undefined) {
        return text.split(/(&[\w#]+;)/).map((/** @type {string} */ part, /** @type {number} */ k) =>
          (k % 2 ? span("cn", part) : escapeHtml(part))).join("");
      }
      if (open === undefined) return escapeHtml(whole);
      const shown = String(attrs ?? "").replace(
        /(\s+)([\w:.@-]+)(?:(\s*=\s*)("[^"]*"|'[^']*'|[^\s>]+))?/g,
        (_, /** @type {string} */ space, /** @type {string} */ attr, /** @type {string | undefined} */ eq,
          /** @type {string | undefined} */ val) =>
          escapeHtml(space) + span("at", attr) + (eq ? span("op", eq) + span("str", val ?? "") : ""),
      );
      const end = close ? escapeHtml(close.slice(0, close.length - close.trimStart().length)) + span("pn", close.trim()) : "";
      return span("pn", open) + span("k", name) + shown + end;
    },
  );
}

/** CSS: selectors, at-rules, properties and their values. @param {string} code */
function lexCss(code) {
  let out = "";
  let depth = 0;
  // At the start of a declaration, where a name and a colon are a property
  // rather than a selector and its pseudo-class.
  let declaration = false;
  const n = code.length;
  let i = 0;
  while (i < n) {
    const c = code[i];
    if (/\s/.test(c)) {
      out += c;
      i++;
      continue;
    }
    if (code.startsWith("/*", i)) {
      const end = code.indexOf("*/", i + 2);
      const stop = end < 0 ? n : end + 2;
      out += span("cm", code.slice(i, stop));
      i = stop;
      continue;
    }
    const rest = code.slice(i, i + 256);
    const starts = declaration;
    declaration = false;
    let m;
    if (c === "{" || c === "}" || c === ";") {
      if (c === "{") depth++;
      if (c === "}") depth = Math.max(0, depth - 1);
      out += span("pn", c);
      declaration = depth > 0;
      i++;
    } else if ((m = rest.match(/^(["'])(?:\\.|(?!\1)[^\\\n])*\1?/))) {
      out += span("str", m[0]);
      i += m[0].length;
    } else if ((m = rest.match(/^@[\w-]+/))) {
      out += span("k", m[0]);
      i += m[0].length;
    } else if (starts && (m = rest.match(/^(--[\w-]+|[\w-]+)(\s*)(:)(?!:)/))) {
      out += span("pr", m[1]) + escapeHtml(m[2]) + span("pn", m[3]);
      i += m[0].length;
    } else if (depth > 0 && (m = rest.match(/^#[\da-fA-F]{3,8}\b/))) {
      out += span("num", m[0]);
      i += m[0].length;
    } else if (!namelike(code[i - 1]) && (m = rest.match(/^-?(?:\d+\.?\d*|\.\d+)(?:%|[a-zA-Z]+)?/))) {
      out += span("num", m[0]);
      i += m[0].length;
    } else if ((m = rest.match(/^([\w-]+)\(/))) {
      out += span("fn", m[1]) + span("pn", "(");
      i += m[0].length;
    } else if ((m = rest.match(/^!important\b/))) {
      out += span("k", m[0]);
      i += m[0].length;
    } else if (depth === 0 && (m = rest.match(/^(?:[.#][\w-]+|::?[\w-]+)/))) {
      out += span(m[0].startsWith(":") ? "fn" : "ty", m[0]);
      i += m[0].length;
    } else if ((m = rest.match(/^[\w-]+/))) {
      out += depth === 0 ? span("k", m[0]) : escapeHtml(m[0]);
      i += m[0].length;
    } else {
      out += /[:,>+~*=()[\]]/.test(c) ? span("pn", c) : escapeHtml(c);
      i++;
    }
  }
  return out;
}

/** A diff: added and removed lines, hunk headers, file headers.
 *  @param {string} code */
function lexDiff(code) {
  return code.split("\n").map((line) => {
    if (/^(\+\+\+|---)(\s|$)/.test(line) || /^(diff |index )/.test(line)) return span("k", line);
    if (line.startsWith("@@")) return span("ty", line);
    if (line.startsWith("+")) return span("da", line);
    if (line.startsWith("-")) return span("dd", line);
    return escapeHtml(line);
  }).join("\n");
}

/** Past this a block is drawn as it is: colouring it would cost more than
 *  it helps, and nobody reads a block that long in a reply. */
const LIMIT = 200_000;

/** The same a line at a time, for a diff, which draws each line in a row
 *  of its own. The code is coloured whole first, so a comment or a string
 *  that spans lines is one; a run still open at the end of a line is closed
 *  there and opened again on the next. (Every run is a flat span, and the
 *  source's own `<` is escaped, so a `<span class="` can only be one.)
 *  @param {string} code @param {string} lang @returns {string[]} */
export function highlightLines(code, lang) {
  let open = "";
  return highlight(code, lang).split("\n").map((line) => {
    let html = open ? `<span class="${open}">${line}` : line;
    const start = html.lastIndexOf('<span class="');
    if (start > html.lastIndexOf("</span>")) {
      open = html.slice(start + 13, html.indexOf('"', start + 13));
      html += "</span>";
    } else open = "";
    return html;
  });
}

/** A block of code as markup: escaped, and coloured if its language is one
 *  this file knows.
 *  @param {string} code @param {string} lang */
export function highlight(code, lang) {
  const known = language(lang);
  if (!known || code.length > LIMIT) return escapeHtml(code);
  switch (known) {
    case "json": return lexJson(code);
    case "yaml": return lexYaml(code);
    case "toml": return lexToml(code);
    case "html": case "xml": return lexMarkup(code);
    case "css": return lexCss(code);
    case "diff": return lexDiff(code);
    default: return lexGeneric(code, LANGS[known]);
  }
}
