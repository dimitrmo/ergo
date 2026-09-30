// Syntax colours for template text: a coloured copy of the text sits behind
// a textarea whose own text is transparent, so typing, selection, undo and
// the Insert chips all stay native.

import { forwardRef, type TextareaHTMLAttributes, useRef } from 'react'

type Token = { t: string; c?: string }

const KEYWORDS = new Set([
  'if', 'elif', 'else', 'endif', 'for', 'in', 'endfor', 'set', 'endset', 'and', 'or', 'not', 'is',
  'true', 'false', 'none', 'True', 'False', 'None', 'loop', 'macro', 'endmacro', 'filter', 'endfilter',
])

/** Inside `{{ }}` / `{% %}`: strings, numbers, keywords, filters, variables. */
function tagTokens(src: string, out: Token[]) {
  const re = /("(?:\\.|[^"\\])*"?|'(?:\\.|[^'\\])*'?)|(\d+(?:\.\d+)?)|(\|\s*[A-Za-z_]\w*)|([A-Za-z_]\w*)|(\s+)|([^\sA-Za-z_\d"'|]+|\|)/g
  for (const m of src.matchAll(re)) {
    const [text, str, num, filter, word] = m
    if (str) out.push({ t: text, c: 'hl-str' })
    else if (num) out.push({ t: text, c: 'hl-num' })
    else if (filter) out.push({ t: text, c: 'hl-filter' })
    else if (word) out.push({ t: text, c: KEYWORDS.has(word) ? 'hl-kw' : 'hl-var' })
    else out.push({ t: text, c: 'hl-op' })
  }
}

/** Plain JSON outside tags: keys, strings, numbers, literals. */
function jsonTokens(src: string, out: Token[]) {
  const re = /("(?:\\.|[^"\\\n])*"?)(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|([{}[\],:])|([^"\d{}[\],:tfn-]+|.)/g
  for (const m of src.matchAll(re)) {
    const [text, str, colon, num, lit, punct] = m
    if (str) {
      out.push({ t: str, c: colon ? 'hl-key' : 'hl-str' })
      if (colon) out.push({ t: colon, c: 'hl-punct' })
    } else if (num) out.push({ t: text, c: 'hl-num' })
    else if (lit) out.push({ t: text, c: 'hl-kw' })
    else if (punct) out.push({ t: text, c: 'hl-punct' })
    else out.push({ t: text })
  }
}

function tokenize(src: string, lang: 'text' | 'json'): Token[] {
  const out: Token[] = []
  const open = /\{\{|\{%|\{#/g
  let at = 0
  for (let m = open.exec(src); m; m = open.exec(src)) {
    const text = src.slice(at, m.index)
    if (lang === 'json') jsonTokens(text, out)
    else if (text) out.push({ t: text })
    const close = { '{{': '}}', '{%': '%}', '{#': '#}' }[m[0]]!
    const end = src.indexOf(close, m.index + 2)
    const inner = src.slice(m.index + 2, end < 0 ? src.length : end)
    if (m[0] === '{#') {
      out.push({ t: m[0] + inner + (end < 0 ? '' : close), c: 'hl-comment' })
    } else {
      out.push({ t: m[0], c: 'hl-delim' })
      tagTokens(inner, out)
      if (end >= 0) out.push({ t: close, c: 'hl-delim' })
    }
    at = end < 0 ? src.length : end + 2
    open.lastIndex = at
  }
  const rest = src.slice(at)
  if (lang === 'json') jsonTokens(rest, out)
  else if (rest) out.push({ t: rest })
  return out
}

type Props = TextareaHTMLAttributes<HTMLTextAreaElement> & { value: string; lang?: 'text' | 'json' }

export const HighlightedTextarea = forwardRef<HTMLTextAreaElement, Props>(function HighlightedTextarea(
  { value, lang = 'text', className, onScroll, ...rest },
  ref,
) {
  const pre = useRef<HTMLPreElement>(null)
  return (
    <div className="hl">
      <pre ref={pre} className="hl-layer mono" aria-hidden="true">
        {tokenize(value, lang).map((tok, i) => (tok.c ? <span key={i} className={tok.c}>{tok.t}</span> : tok.t))}
        {/* A trailing newline needs a line of its own to keep heights equal. */}
        {'\n '}
      </pre>
      <textarea
        ref={ref}
        className={`${className ?? ''} hl-input`}
        value={value}
        onScroll={(e) => {
          if (pre.current) pre.current.scrollTop = e.currentTarget.scrollTop
          onScroll?.(e)
        }}
        {...rest}
      />
    </div>
  )
})
