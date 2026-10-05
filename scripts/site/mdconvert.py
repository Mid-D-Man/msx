#!/usr/bin/env python3
"""
mdconvert.py
A small, purpose-built Markdown to HTML converter for the MSX docs site.

Not CommonMark. It handles the constructs the docs actually use: ATX
headings (#..####), fenced code blocks, GFM pipe tables, flat bullet and
numbered lists (indented continuation lines join the item above), block
quotes used as callouts, horizontal rules, inline code, bold, italic,
inline links, and an HTML-comment directive for site-only content.
No external dependency, so CI needs no `pip install`.

Callouts: a block quote whose first word is bold becomes a styled box,
for example `> **Note** text`. The bold word picks the style (note, tip,
warning, limitation).

Directives: a line holding only `<!-- kind: argument -->` is passed to the
`directive` callback and replaced by whatever HTML it returns. GitHub
renders such a line as nothing, so the same files read cleanly there.

Links: `[text](target)` goes through the optional `link_rewrite` callback,
which lets the site map relative `page.md` links onto its own URLs.

Headings get a stable `id` (slugified text, numbered when repeated) for
deep links and the generated table of contents.
"""

import html
import re


def _slugify(text: str) -> str:
    s = re.sub(r"[^\w\s-]", "", text.lower())
    s = re.sub(r"[\s_]+", "-", s).strip("-")
    return s or "section"


def _inline(text: str, link_rewrite=None) -> str:
    """Escape first, then apply inline code, links, bold and italic in that
    order, so markup characters inside a code span are never reinterpreted."""
    text = html.escape(text, quote=False)

    spans = []

    def stash_code(m):
        spans.append(m.group(1))
        return f"\x00{len(spans) - 1}\x00"

    text = re.sub(r"`([^`]+)`", stash_code, text)

    def link(m):
        label, target = m.group(1), m.group(2)
        if link_rewrite is not None:
            target = link_rewrite(html.unescape(target))
        return f'<a href="{html.escape(target, quote=True)}">{label}</a>'

    text = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", link, text)

    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"(?<!\*)\*([^*]+)\*(?!\*)", r"<em>\1</em>", text)

    def restore_code(m):
        return f"<code>{spans[int(m.group(1))]}</code>"

    return re.sub(r"\x00(\d+)\x00", restore_code, text)


_ORDERED = re.compile(r"^(\d+)\.\s+(.*)$")
_BULLET = re.compile(r"^[-*]\s+(.*)$")
_DIRECTIVE = re.compile(r"^<!--\s*(\w+)\s*:\s*(.+?)\s*-->\s*$")


def convert(md: str, link_rewrite=None, directive=None) -> tuple[str, list[dict]]:
    """Returns (body_html, toc); toc lists every h2/h3 as {level, id, text}."""
    lines = md.replace("\r\n", "\n").split("\n")
    out = []
    toc = []
    seen_slugs: dict[str, int] = {}
    i = 0
    n = len(lines)

    def inl(t):
        return _inline(t, link_rewrite)

    list_buf: list[str] = []
    list_kind = [""]
    para_buf: list[str] = []

    def flush_list():
        if list_buf:
            tag = "ol" if list_kind[0] == "ol" else "ul"
            out.append(f"<{tag}>")
            for item in list_buf:
                out.append(f"<li>{inl(item)}</li>")
            out.append(f"</{tag}>")
            list_buf.clear()
        list_kind[0] = ""

    def flush_para():
        if para_buf:
            out.append(f"<p>{inl(' '.join(para_buf))}</p>")
            para_buf.clear()

    while i < n:
        line = lines[i]

        # Fenced code block
        m = re.match(r"^```(\w*)\s*$", line)
        if m:
            flush_list()
            flush_para()
            lang = m.group(1) or "text"
            i += 1
            code_lines = []
            while i < n and not re.match(r"^```\s*$", lines[i]):
                code_lines.append(lines[i])
                i += 1
            i += 1
            code = html.escape("\n".join(code_lines))
            out.append(f'<pre class="code lang-{lang}"><code>{code}</code></pre>')
            continue

        # Site directive on its own line
        m = _DIRECTIVE.match(line)
        if m:
            flush_list()
            flush_para()
            if directive is not None:
                out.append(directive(m.group(1), m.group(2)))
            i += 1
            continue

        # ATX heading
        m = re.match(r"^(#{1,4})\s+(.*)$", line)
        if m:
            flush_list()
            flush_para()
            level = len(m.group(1))
            text = m.group(2).strip()
            slug = _slugify(re.sub(r"`", "", text))
            count = seen_slugs.get(slug, 0)
            seen_slugs[slug] = count + 1
            if count:
                slug = f"{slug}-{count + 1}"
            out.append(f'<h{level} id="{slug}">{inl(text)}</h{level}>')
            if level in (2, 3):
                toc.append({"level": level, "id": slug, "text": text})
            i += 1
            continue

        # Horizontal rule
        if re.match(r"^-{3,}\s*$", line):
            flush_list()
            flush_para()
            out.append("<hr>")
            i += 1
            continue

        # Block quote, used as a callout
        if line.startswith(">"):
            flush_list()
            flush_para()
            quote: list[str] = []
            while i < n and lines[i].startswith(">"):
                quote.append(lines[i][1:].lstrip())
                i += 1
            paragraphs, cur = [], []
            for q in quote:
                if q.strip() == "":
                    if cur:
                        paragraphs.append(" ".join(cur))
                        cur = []
                else:
                    cur.append(q.strip())
            if cur:
                paragraphs.append(" ".join(cur))
            kind = "note"
            head = re.match(r"^\*\*(\w+)\*\*", paragraphs[0]) if paragraphs else None
            if head:
                kind = head.group(1).lower()
            body = "".join(f"<p>{inl(p)}</p>" for p in paragraphs)
            out.append(f'<div class="callout callout-{kind}">{body}</div>')
            continue

        # Pipe table
        if "|" in line and i + 1 < n and re.match(r"^\s*\|?[\s:|-]+\|[\s:|-]*$", lines[i + 1]):
            flush_list()
            flush_para()
            header_cells = [c.strip() for c in line.strip().strip("|").split("|")]
            out.append('<div class="table-wrap"><table class="doc-table"><thead><tr>')
            for c in header_cells:
                out.append(f"<th>{inl(c)}</th>")
            out.append("</tr></thead><tbody>")
            i += 2
            while i < n and "|" in lines[i]:
                row_cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                out.append("<tr>")
                for c in row_cells:
                    out.append(f"<td>{inl(c)}</td>")
                out.append("</tr>")
                i += 1
            out.append("</tbody></table></div>")
            continue

        # List items
        m_ol = _ORDERED.match(line)
        m_ul = _BULLET.match(line)
        if m_ol or m_ul:
            flush_para()
            kind = "ol" if m_ol else "ul"
            if list_buf and list_kind[0] != kind:
                flush_list()
            list_kind[0] = kind
            list_buf.append((m_ol or m_ul).group(2 if m_ol else 1))
            i += 1
            continue

        # Indented continuation of the previous list item
        if list_buf and re.match(r"^\s{2,}\S", line):
            list_buf[-1] += " " + line.strip()
            i += 1
            continue

        # Blank line
        if line.strip() == "":
            flush_list()
            flush_para()
            i += 1
            continue

        flush_list()
        para_buf.append(line.strip())
        i += 1

    flush_list()
    flush_para()
    return "\n".join(out), toc


if __name__ == "__main__":
    import sys
    src = open(sys.argv[1]).read()
    body, toc = convert(src)
    print(body)
    print("\n<!-- TOC:", toc, "-->", file=sys.stderr)
