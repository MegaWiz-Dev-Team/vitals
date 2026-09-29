#!/usr/bin/env python3
"""Build vitals.academy/blog from posts.json.

    python3 crates/vitals-web/blog/build.py

Writes static/blog/*.html and src/blog.rs. Each post's words are the words published on
Colosseum, unedited; a figure we later found wrong gets a dated correction under it, never a
silent rewrite. Images stay in blog/img and are compiled in from there.
"""
import datetime, html, json, pathlib, re, shutil

HERE = pathlib.Path(__file__).resolve().parent
CRATE = HERE.parent
OUT = CRATE / "static" / "blog"
POSTS = json.loads((HERE / "posts.json").read_text())
POSTS.sort(key=lambda p: p["published"])

HEAD = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{desc}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{desc}">
{og_image}<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>🫀</text></svg>">
<link rel="stylesheet" href="/fonts/fonts.css">
<style>
:root{{--ground:#F5F8F8;--surface:#FFFFFF;--ink:#0F1719;--ink-2:#48585C;--ink-3:#7E8F93;
  --rule:#D9E2E2;--rule-soft:#E8EEEE;--proven:#0B6E58;--attested:#A24714;--attested-soft:#F6E9E1}}
@media (prefers-color-scheme:dark){{:root{{--ground:#0B1113;--surface:#121B1D;--ink:#E9F0F0;--ink-2:#9CAEB1;
  --ink-3:#68797D;--rule:#223033;--rule-soft:#1A2528;--proven:#42D2A7;--attested:#E5924F;--attested-soft:#2A1C12}}}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--ground);color:var(--ink);font-family:"IBM Plex Sans","Helvetica Neue",Arial,sans-serif;
  font-size:17px;line-height:1.7;-webkit-font-smoothing:antialiased}}
a{{color:var(--proven);overflow-wrap:anywhere}}
h1,h2{{font-family:Archivo,Arial,sans-serif;margin:0;text-wrap:balance;line-height:1.15}}
.wrap{{max-width:44rem;margin:0 auto;padding-inline:clamp(1rem,4vw,2rem)}}
.rail{{border-bottom:1px solid var(--rule-soft);padding-block:.8rem;font-family:"IBM Plex Mono",monospace;font-size:.72rem;
  letter-spacing:.1em;color:var(--ink-3);display:flex;gap:1rem;flex-wrap:wrap}}
.rail a{{color:var(--ink);text-decoration:none;font-weight:600;letter-spacing:.14em}}
.meta{{font-family:"IBM Plex Mono",monospace;font-size:.78rem;color:var(--ink-3);letter-spacing:.04em}}
header.post{{padding-block:clamp(1.8rem,6vw,3.2rem) 1.2rem}}
header.post h1{{font-size:clamp(1.8rem,5vw,2.6rem);font-weight:800;margin-block:.5rem}}
article p{{margin:0 0 1.1rem}}
figure{{margin:1.6rem 0}}
figure img{{display:block;max-width:100%;height:auto;border:1px solid var(--rule);border-radius:8px;background:var(--surface)}}
figcaption{{font-size:.82rem;color:var(--ink-3);margin-top:.4rem}}
.fix{{background:var(--attested-soft);border-left:3px solid var(--attested);padding:.8rem 1rem;border-radius:4px;
  font-size:.92rem;margin:1.6rem 0}}
.pager{{display:flex;justify-content:space-between;gap:1rem;border-top:1px solid var(--rule);margin-top:2.4rem;
  padding-top:1.2rem;font-size:.92rem;flex-wrap:wrap}}
.list{{list-style:none;margin:0;padding:0}}
.list li{{border-bottom:1px solid var(--rule-soft);padding-block:1.1rem}}
.list h2{{font-size:1.25rem;margin:.25rem 0 .35rem}}
.list h2 a{{color:var(--ink);text-decoration:none}}
.list h2 a:hover{{color:var(--proven)}}
.list p{{margin:0;color:var(--ink-2);font-size:.95rem}}
.tag{{font-family:"IBM Plex Mono",monospace;font-size:.68rem;letter-spacing:.1em;text-transform:uppercase;color:var(--proven)}}
footer{{border-top:1px solid var(--rule-soft);margin-top:3rem;padding-block:1.4rem 2.4rem;font-size:.82rem;color:var(--ink-3)}}
footer a{{margin-right:1rem}}
</style>
</head>
<body>
<div class="wrap">
<div class="rail"><a href="/">VITALS</a><a href="/blog">BLOG</a><span>builder updates, as published</span></div>
"""

FOOT = """<footer>
<div><a href="/">Vitals</a><a href="/blog">Blog</a><a href="https://world.vitals.academy/?src=blog" rel="noopener">Vitals World</a><a href="/privacy">Privacy</a><a href="/terms">Terms of use</a></div>
<p>Every post here was first published as a builder update on Colosseum and is reproduced as it was written. Where we later found a figure was wrong, a dated correction sits under the post. The patients in Vitals World are simulated; the cases are for practice and are not medical advice.</p>
<p>Built by MegaWiz Co., Ltd. (Bangkok, Thailand) · paripol@megawiz.co</p>
</footer>
</div>
</body>
</html>
"""

URL = re.compile(r"(https?://[^\s<]+|(?<![\w/.@-])(?:world\.vitals\.academy|vitals\.academy)(?:/[^\s<]*)?)")


def href_for(u: str) -> str:
    full = u if u.startswith("http") else "https://" + u
    # The ward counts arrivals per link we hand out; a link read on the blog says so.
    if "world.vitals.academy" in full:
        full = re.sub(r"([?&])src=[^&]*", r"\1src=blog", full)
        if "src=" not in full:
            full += ("&" if "?" in full else ("?" if full.count("/") > 2 else "/?")) + "src=blog"
    return full


def linkify(text: str) -> str:
    out, last = [], 0
    for m in URL.finditer(text):
        raw = m.group(0)
        u = raw.rstrip(".,;)")
        out.append(html.escape(text[last:m.start()]))
        out.append(f'<a href="{html.escape(href_for(u))}" rel="noopener">{html.escape(u)}</a>')
        out.append(html.escape(raw[len(u):]))
        last = m.end()
    out.append(html.escape(text[last:]))
    return "".join(out)


def body(p) -> list:
    """The post's paragraphs, without an opening headline that is already the page's title."""
    norm = lambda t: re.sub(r"[^a-z0-9]+", " ", t.lower()).strip()
    if p["paras"] and norm(p["paras"][0]) == norm(p["title"]):
        return p["paras"][1:]
    return p["paras"]


def when(p) -> datetime.datetime:
    return datetime.datetime.fromisoformat(p["published"])


def day(p) -> str:
    return when(p).strftime("%-d %B %Y")


def lede(p, n=180) -> str:
    paras = body(p)
    t = " ".join(paras[0].split())
    if len(t) < 60 and len(paras) > 1:
        t += " " + " ".join(paras[1].split())
    return t if len(t) <= n else t[: n - 1].rsplit(" ", 1)[0] + "…"


def where(p) -> str:
    return "Colosseum Eternal" if p["id"] == "eternal" else "Vitals World · Crypto World's Fair"


def post_page(i, p) -> str:
    og = f'<meta property="og:image" content="https://vitals.academy/blog/img/{p["images"][0]["file"]}">\n' if p["images"] else ""
    h = HEAD.format(title=html.escape(p["title"] + " · Vitals blog"), desc=html.escape(lede(p)), og_image=og)
    ref = "the project page" if p["id"] == "eternal" else f"builder update #{p['id']}"
    h += '<article>\n<header class="post">\n'
    h += f'<div class="tag">{html.escape(where(p))}</div>\n<h1>{html.escape(p["title"])}</h1>\n'
    h += (f'<div class="meta"><time datetime="{p["published"]}">{day(p)}, {when(p).strftime("%H:%M")} ICT</time>'
          f' · first published as <a href="{html.escape(p["source"])}" rel="noopener">{ref} on Colosseum</a></div>\n')
    h += "</header>\n"
    for k, para in enumerate(body(p)):
        h += f"<p>{linkify(para).replace(chr(10), '<br>')}</p>\n"
        if k == 0 and p["images"]:
            for im in p["images"]:
                h += (f'<figure><img src="/blog/img/{im["file"]}" alt="{html.escape(im["alt"])}" loading="lazy">'
                      f'<figcaption>{html.escape(im["alt"])}</figcaption></figure>\n')
    fixes = p.get("correction") or []
    for fix in [fixes] if isinstance(fixes, str) else fixes:
        h += f'<div class="fix">{html.escape(fix)}</div>\n'
    h += "</article>\n<nav class=\"pager\">"
    h += f'<a href="/blog/{POSTS[i-1]["slug"]}">← {html.escape(POSTS[i-1]["title"])}</a>' if i > 0 else "<span></span>"
    h += f'<a href="/blog/{POSTS[i+1]["slug"]}">{html.escape(POSTS[i+1]["title"])} →</a>' if i + 1 < len(POSTS) else '<a href="/blog">All posts</a>'
    h += "</nav>\n"
    return h + FOOT


def index_page() -> str:
    first = next(p for p in reversed(POSTS) if p["images"])
    og = f'<meta property="og:image" content="https://vitals.academy/blog/img/{first["images"][0]["file"]}">\n'
    h = HEAD.format(title="Vitals blog: builder updates", og_image=og,
                    desc="Builder updates from Vitals and Vitals World, as we published them on Colosseum, with corrections where we got a figure wrong.")
    h += ('<header class="post"><h1>Builder updates</h1>\n<p class="meta">Vitals and Vitals World, as we published them on '
          'Colosseum. Newest first.</p></header>\n<ul class="list">\n')
    for p in reversed(POSTS):
        h += (f'<li><div class="tag">{html.escape(where(p))} · <time datetime="{p["published"]}">{day(p)}</time></div>'
              f'<h2><a href="/blog/{p["slug"]}">{html.escape(p["title"])}</a></h2><p>{html.escape(lede(p))}</p></li>\n')
    return h + "</ul>\n" + FOOT


NOT_FOUND = (HEAD.format(title="Not found · Vitals blog", desc="No post at this address.", og_image="")
             + '<header class="post"><h1>No post at this address</h1></header>\n<p><a href="/blog">All posts</a></p>\n' + FOOT)


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)
    pages = [("/blog", "index.html", index_page())]
    for i, p in enumerate(POSTS):
        pages.append((f"/blog/{p['slug']}", f"{p['slug']}.html", post_page(i, p)))
    for _, name, body in pages:
        (OUT / name).write_text(body)
    (OUT / "not-found.html").write_text(NOT_FOUND)
    types = {"jpg": "image/jpeg", "gif": "image/gif", "png": "image/png"}
    images = []
    for p in POSTS:
        for im in p["images"]:
            images.append(im["file"])
    rs = ["//! vitals.academy/blog — generated by `crates/vitals-web/blog/build.py` from `posts.json`.",
          "//! Edit the posts there and rebuild; this file is overwritten.", "",
          "pub const PAGES: &[(&str, &str)] = &["]
    rs += [f'    ("{route}", include_str!("../static/blog/{name}")),' for route, name, _ in pages]
    rs += ["];", "", "pub const IMAGES: &[(&str, &[u8], &str)] = &["]
    rs += [f'    ("/blog/img/{f}", include_bytes!("../blog/img/{f}"), "{types[f.rsplit(".", 1)[1]]}"),' for f in images]
    rs += ["];", "", 'pub const NOT_FOUND: &str = include_str!("../static/blog/not-found.html");', ""]
    rs += ["pub enum Page {", "    Html(&'static str),", "    Image(&'static [u8], &'static str),", "}", "",
           "/// Whether a path belongs to the blog: `/blog` and everything under `/blog/`, nothing else.",
           "pub fn is_blog(path: &str) -> bool {",
           '    path == "/blog" || path.starts_with("/blog/")', "}", "",
           "/// The page or image at a blog path, with or without a trailing slash; `None` is a 404.",
           "pub fn serve(path: &str) -> Option<Page> {",
           "    let path = match path.strip_suffix('/') {", '        Some(p) if !p.is_empty() => p,', "        _ => path,", "    };",
           "    PAGES.iter().find(|(p, _)| *p == path).map(|(_, h)| Page::Html(h)).or_else(|| {",
           "        IMAGES.iter().find(|(p, ..)| *p == path).map(|(_, b, t)| Page::Image(b, t))", "    })", "}", ""]
    (CRATE / "src" / "blog.rs").write_text("\n".join(rs))
    print(f"{len(pages)} pages, {len(images)} images")


if __name__ == "__main__":
    main()
