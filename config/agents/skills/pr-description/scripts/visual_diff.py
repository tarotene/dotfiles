#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright==1.63.0", "pillow==11.3.0"]  # playwright は apply-offer-plan と同じ版。上げるときは --selftest --browser を通す
# ///
"""visual_diff.py — 見た目の Before / After / Diff を、固定の撮り方で撮り、並べる(ADR-1698 の執行点)。

  capture  撮影定義(visual.toml)の全セル(面 × 状態 × 明暗 × 幅)を撮る。画面のファイルは --ref から取り出す
  diff     2 つの撮影ディレクトリを比べ、セルごとに Before | After | Diff の 1 枚と report.json を出す
  page     report.json から、本人に見せる並べたページ(index.html)を作る
  audit    面が宣言した型(types)の要件のうち、描画した HTML で測れるものを検査する(page-types.toml。ADR-1701)

固定の手順は pr-description スキル §5b。要点:
  - Before は **基準の ref から取り出した**ファイルで撮る。作業ツリーからは撮れない(変更後に Before を撮れない形)。
    撮影定義と疑似(stub)は、Before・After とも現在の作業ツリーのものを使う(疑似の違いを画面の差に混ぜない)。
  - 撮る行列は、明・暗 × 幅 390px・1280px。状態は撮影定義が宣言する。
  - 第三者への要求は遮断する(決定的にするためで、ADR-1677 D6 と同じ線)。遮断した URL は manifest に残す。
  - 画面に差し込むデータは合成データだけ。実データを撮影定義に置かない。

撮影定義 visual.toml(キーは閉じている。知らないキーは誤り):

  [[page]]
  id = "today"                     # 面の識別子([a-z0-9-]+)
  root = "state/reviews/daily"     # リポジトリの根からの相対。この下を配信の根にする
  entry = "today-page.html"        # root からの相対(prepare の出力でもよい)
  prepare = "bash build.sh ..."    # 任意。取り出したツリーの中(cwd = root)で実行する
  stub = "visual/stub.js"          # 任意。撮影定義からの相対。ページのスクリプトより前に差し込む(window.claude の疑似など)
  host = "artifact"                # "artifact" = ホストが付ける data-theme を再現 / "site"(既定) = 付けない
  clock = "2026-11-24T09:00:00+09:00"   # 任意。Date を固定する
  types = ["common", "form"]       # 任意。当てはまるページの型(page-types.toml の id)。省略は ["common"]
  [[page.state]]
  name = "failed"                  # 状態の名前([a-z0-9-]+)
  globals = { mode = "fail" }      # 任意。window.__VISUAL__ に入る(疑似が読む)
  steps = [ { click = "#go" }, { press = "Tab" }, { fill = "#q", value = "x" }, { wait = 300 } ]
"""
from __future__ import annotations

import argparse
import html
import io
import json
import mimetypes
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path
from urllib.parse import unquote, urlparse

THEMES = ("light", "dark")
WIDTHS = (390, 1280)
HOST = "http://visual.test"
DEFAULT_CLOCK = "2026-01-01T09:00:00+09:00"
PAD_FILL = (255, 0, 255)  # 高さが違うとき、短い側を埋める色(ページの内容と取り違えない色)
NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
PAGE_KEYS = {"id", "root", "entry", "prepare", "stub", "host", "clock", "state", "types"}
CHECKS = ("audit", "walkthrough", "visual")
# 描画した HTML への検査(閉じた語彙)。page-types.toml の要件の `audit` は、この中から選ぶ。
AUDITS = (
    "document", "contrast", "targets", "overflow", "third-party",
    "keyboard", "main-button", "default-choice", "confirm", "links",
)
REQ_ID_RE = re.compile(r"^[CLFD][0-9]+$")
STATE_KEYS = {"name", "globals", "steps"}
STEP_KEYS = {"click", "press", "fill", "value", "wait"}


class SpecError(Exception):
    pass


# ── 撮影定義 ──────────────────────────────────────────────────────────────────

def load_spec(path: Path) -> list[dict]:
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise SpecError(f"{path}: 読めません({e})") from e
    extra = set(data) - {"page"}
    if extra:
        raise SpecError(f"{path}: 知らないキー {sorted(extra)}")
    pages = data.get("page") or []
    if not pages:
        raise SpecError(f"{path}: [[page]] がありません")
    seen: set[str] = set()
    for p in pages:
        extra = set(p) - PAGE_KEYS
        if extra:
            raise SpecError(f"{path}: page に知らないキー {sorted(extra)}")
        for k in ("id", "root", "entry"):
            if not isinstance(p.get(k), str) or not p[k]:
                raise SpecError(f"{path}: page に {k} がありません")
        if not NAME_RE.match(p["id"]):
            raise SpecError(f"{path}: page.id は [a-z0-9-]+ です: {p['id']!r}")
        if p["id"] in seen:
            raise SpecError(f"{path}: page.id が重複しています: {p['id']}")
        seen.add(p["id"])
        if p.get("host", "site") not in ("site", "artifact"):
            raise SpecError(f"{path}: host は site か artifact です: {p['host']!r}")
        types = p.get("types", ["common"])
        if not isinstance(types, list) or not types or not all(isinstance(t, str) and NAME_RE.match(t) for t in types):
            raise SpecError(f"{path}: types は、型の id([a-z0-9-]+)の配列です: {types!r}")
        root = Path(p["root"])
        if root.is_absolute() or ".." in root.parts or p["root"] in (".", ""):
            raise SpecError(f"{path}: root は、リポジトリの中の下位のディレクトリです: {p['root']!r}")
        states = p.get("state") or [{"name": "initial"}]
        names: set[str] = set()
        for s in states:
            extra = set(s) - STATE_KEYS
            if extra:
                raise SpecError(f"{path}: state に知らないキー {sorted(extra)}")
            if not NAME_RE.match(str(s.get("name", ""))):
                raise SpecError(f"{path}: state.name は [a-z0-9-]+ です: {s.get('name')!r}")
            if s["name"] in names:
                raise SpecError(f"{path}: state.name が重複しています: {p['id']}/{s['name']}")
            names.add(s["name"])
            for st in s.get("steps", []):
                extra = set(st) - STEP_KEYS
                kinds = [k for k in ("click", "press", "fill", "wait") if k in st]
                if extra or len(kinds) != 1 or ("fill" in st) != ("value" in st):
                    raise SpecError(f"{path}: step の形が違います: {st}")
        p["state"] = states
    return pages


def cell_name(page: str, state: str, theme: str, width: int) -> str:
    return f"{page}--{state}--{theme}--{width}"


# ── 基準の ref からの取り出し ─────────────────────────────────────────────────

def repo_root(start: Path) -> Path:
    out = subprocess.run(
        ["git", "-C", str(start), "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    return Path(out)


def materialize(repo: Path, root_rel: str, ref: str, dest: Path) -> None:
    """root_rel の中身を dest に置く。ref が WORKTREE なら作業ツリーの複製、それ以外は git の ref から取り出す。"""
    if ref == "WORKTREE":
        import shutil

        shutil.copytree(repo / root_rel, dest, ignore=shutil.ignore_patterns(".git"))
        return
    raw = subprocess.run(
        ["git", "-C", str(repo), "archive", "--format=tar", ref, "--", root_rel],
        capture_output=True, check=True,
    ).stdout
    dest.mkdir(parents=True, exist_ok=True)
    prefix = root_rel.rstrip("/") + "/"
    with tarfile.open(fileobj=io.BytesIO(raw)) as tf:
        members = []
        for m in tf.getmembers():
            if not m.name.startswith(prefix) or m.name == prefix:
                continue
            m.name = m.name[len(prefix):]
            members.append(m)
        tf.extractall(dest, members=members, filter="data")


# ── 撮影 ──────────────────────────────────────────────────────────────────────

THEME_SCRIPT = """(function (t) {
  var set = function () { document.documentElement.setAttribute('data-theme', t); };
  if (document.documentElement) set();
  else new MutationObserver(function (_, o) { if (document.documentElement) { set(); o.disconnect(); } })
    .observe(document, { childList: true });
})(%s);"""


def _router(site: Path, blocked: list[str]):
    site = site.resolve()

    def handler(route):
        url = route.request.url
        if url.startswith(HOST + "/"):
            f = (site / unquote(urlparse(url).path.lstrip("/"))).resolve()
            if f.is_file() and site in f.parents:
                ctype = mimetypes.guess_type(f.name)[0] or "application/octet-stream"
                route.fulfill(body=f.read_bytes(), content_type=ctype + ("; charset=utf-8" if ctype.startswith("text/") else ""))
            else:
                route.fulfill(status=404, body="")
        elif url.startswith(("data:", "blob:", "about:")):
            route.continue_()
        else:
            blocked.append(url)
            route.abort()

    return handler


def _run_steps(page, steps: list[dict]) -> None:
    for st in steps:
        if "click" in st:
            page.locator(st["click"]).first.click(timeout=5000)
        elif "press" in st:
            page.keyboard.press(st["press"])
        elif "fill" in st:
            page.locator(st["fill"]).first.fill(st["value"], timeout=5000)
        elif "wait" in st:
            page.wait_for_timeout(min(int(st["wait"]), 2000))


def _walk(spec_path: Path, ref: str, only: str | None):
    """撮影定義の面 × 状態を、ブラウザと配信ツリーつきで順に出す。"""
    from playwright.sync_api import sync_playwright

    spec_path = spec_path.resolve()
    pages = load_spec(spec_path)
    repo = repo_root(spec_path.parent)
    with tempfile.TemporaryDirectory(prefix="visual-diff-") as tmp, sync_playwright() as pw:
        browser = pw.chromium.launch(chromium_sandbox=False)
        try:
            for p in pages:
                if only and only.split(":")[0] != p["id"]:
                    continue
                site = Path(tmp) / p["id"]
                materialize(repo, p["root"], ref, site)
                if p.get("prepare"):
                    subprocess.run(["bash", "-c", p["prepare"]], cwd=site, check=True)
                if not (site / p["entry"]).is_file():
                    raise SpecError(f"{p['id']}: entry が無い({p['entry']}。ref={ref})")
                stub = (spec_path.parent / p["stub"]).read_text(encoding="utf-8") if p.get("stub") else ""
                for s in p["state"]:
                    if only and ":" in only and only.split(":", 1)[1] != s["name"]:
                        continue
                    yield browser, site, p, s, stub
        finally:
            browser.close()


def capture(spec_path: Path, ref: str, out: Path, only: str | None = None, measure: bool = False) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    manifest: dict = {"ref": ref, "spec": str(spec_path.resolve()), "cells": []}
    for browser, site, p, s, stub in _walk(spec_path, ref, only):
        for theme in THEMES:
            for width in WIDTHS:
                manifest["cells"].append(_shoot(browser, site, p, s, theme, width, stub, out, measure))
    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    return manifest


def _open(browser, site: Path, p: dict, s: dict, theme: str, width: int, stub: str):
    """面の状態を開いた(状態の操作を終えた)ページを返す。呼び出し側が ctx.close() する。"""
    ctx = browser.new_context(
        viewport={"width": width, "height": 800},
        color_scheme=theme,
        reduced_motion="reduce",
        locale="ja-JP",
        timezone_id="Asia/Tokyo",
        device_scale_factor=1,
    )
    blocked: list[str] = []
    errors: list[str] = []
    dialogs: list[str] = []
    ctx.route("**/*", _router(site, blocked))
    page = ctx.new_page()
    page.on("pageerror", lambda e: errors.append(str(e)[:200]))
    page.on("dialog", lambda d: (dialogs.append(d.type), d.dismiss()))
    page.clock.set_fixed_time(p.get("clock", DEFAULT_CLOCK))
    if p.get("host") == "artifact":
        page.add_init_script(THEME_SCRIPT % json.dumps(theme))
    page.add_init_script("window.__VISUAL__ = %s;" % json.dumps(s.get("globals", {})))
    if stub:
        page.add_init_script(stub)
    page.goto(f"{HOST}/{p['entry']}", wait_until="load")
    page.evaluate("document.fonts ? document.fonts.ready.then(() => true) : true")
    _run_steps(page, s.get("steps", []))
    page.wait_for_timeout(200)
    return ctx, page, blocked, errors, dialogs


def _shoot(browser, site: Path, p: dict, s: dict, theme: str, width: int, stub: str, out: Path, measure: bool = False) -> dict:
    name = cell_name(p["id"], s["name"], theme, width)
    ctx, page, blocked, errors, dialogs = _open(browser, site, p, s, theme, width, stub)
    try:
        overflow = page.evaluate("document.documentElement.scrollWidth > window.innerWidth + 1")
        measures = page.evaluate(AUDIT_JS) if measure else None
        page.screenshot(path=str(out / f"{name}.png"), full_page=True, animations="disabled", caret="hide")
        height = page.evaluate("document.documentElement.scrollHeight")
    finally:
        ctx.close()
    cell = {
        "cell": name, "page": p["id"], "state": s["name"], "theme": theme, "width": width,
        "overflow_x": bool(overflow), "height": int(height),
        "blocked": sorted(set(blocked)), "page_errors": errors, "dialogs": dialogs,
    }
    if measures is not None:
        cell["measures"] = measures
    return cell


# ── 型の要件の検査(audit) ───────────────────────────────────────────────────

# 描画したページの中で動かす測定。コントラストは、文字を持つ要素ごとに、文字色と背景色(祖先をたどって重ねる)から WCAG 2.2 の式で出す。
AUDIT_JS = """() => {
  const cv = document.createElement('canvas'); cv.width = cv.height = 1;
  const cx = cv.getContext('2d', { willReadFrequently: true });
  const rgba = (css) => { cx.clearRect(0,0,1,1); cx.fillStyle = '#000'; cx.fillStyle = css; cx.fillRect(0,0,1,1); const d = cx.getImageData(0,0,1,1).data; return [d[0], d[1], d[2], d[3]/255]; };
  const lin = (c) => { c /= 255; return c <= 0.03928 ? c/12.92 : Math.pow((c+0.055)/1.055, 2.4); };
  const lum = (c) => 0.2126*lin(c[0]) + 0.7152*lin(c[1]) + 0.0722*lin(c[2]);
  const ratio = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x,y)+0.05)/(Math.min(x,y)+0.05); };
  const over = (top, bot) => { const a = top[3] + bot[3]*(1-top[3]); if (a === 0) return [0,0,0,0]; const f = (i) => (top[i]*top[3] + bot[i]*bot[3]*(1-top[3]))/a; return [f(0), f(1), f(2), a]; };
  const probe = document.createElement('div'); probe.style.cssText = 'position:fixed;left:-9999px;background-color:Canvas'; document.body.appendChild(probe);
  const canvasColor = rgba(getComputedStyle(probe).backgroundColor); probe.remove();
  const visible = (el) => { const r = el.getBoundingClientRect(); if (!r.width || !r.height) return false; const s = getComputedStyle(el); return s.visibility !== 'hidden' && s.display !== 'none'; };
  const bgOf = (el) => { const chain = []; for (let e = el; e; e = e.parentElement) { const c = rgba(getComputedStyle(e).backgroundColor); chain.push(c); if (c[3] >= 0.99) break; } let acc = canvasColor; for (let i = chain.length-1; i >= 0; i--) acc = over(chain[i], acc); return acc; };
  const label = (el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\s+/)[0] : '');
  const texts = [], seen = new Set(), walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) { const n = walker.currentNode; if (!n.nodeValue.trim()) continue; const el = n.parentElement;
    if (!el || seen.has(el) || ['SCRIPT','STYLE','NOSCRIPT','OPTION'].includes(el.tagName) || !visible(el)) continue; seen.add(el); texts.push(el); }
  const fails = []; let failCount = 0;
  for (const el of texts) { const s = getComputedStyle(el); if (el.closest('[disabled]')) continue;
    const fg0 = rgba(s.color), bg = bgOf(el); let op = 1; for (let e = el; e; e = e.parentElement) op *= parseFloat(getComputedStyle(e).opacity);
    const r = ratio(over([fg0[0], fg0[1], fg0[2], fg0[3]*op], bg), bg);
    const px = parseFloat(s.fontSize), large = px >= 24 || (px >= 18.66 && parseInt(s.fontWeight, 10) >= 700);
    if (r < (large ? 3 : 4.5)) { failCount++; if (fails.length < 8) fails.push({ el: label(el), text: el.textContent.trim().slice(0, 24), ratio: Math.round(r*100)/100, px: px }); } }
  const small = [], smallLinks = [];
  for (const el of document.querySelectorAll('button, a[href], input:not([type=hidden]), select, textarea, summary, [role=button], [role=tab], [role=radio], [role=checkbox]')) {
    if (!visible(el) || el.disabled) continue; const s = getComputedStyle(el);
    if (el.tagName === 'A' && s.display === 'inline' && el.parentElement && el.parentElement.textContent.trim().length > el.textContent.trim().length + 8) continue;
    // ラベルで囲まれた(ラベルに結びついた)ラジオ・チェックボックスは、押せる範囲がラベルの行全体になる(WCAG 2.5.8 の「対象」)
    const r = (el.tagName === 'INPUT' && el.labels && el.labels.length) ? el.labels[0].getBoundingClientRect() : el.getBoundingClientRect(), m = Math.min(r.width, r.height);
    if (m < 24) (el.tagName === 'A' ? smallLinks : small).push({ el: label(el), w: Math.round(r.width), h: Math.round(r.height) }); }
  return {
    lang: document.documentElement.lang || '', title: (document.title || '').trim(), viewport: !!document.querySelector('meta[name=viewport]'),
    mains: document.querySelectorAll('main, [role=main]').length, h1s: document.querySelectorAll('h1').length,
    contrastFailCount: failCount, contrastFails: fails,
    smallControlCount: small.length, smallControls: small.slice(0, 6), smallLinkCount: smallLinks.length, smallLinks: smallLinks.slice(0, 6),
    mainButtons: Array.from(document.querySelectorAll('.btn-main')).filter(visible).length,
    checkedRadios: document.querySelectorAll('input[type=radio]:checked').length,
  };
}"""

KEYBOARD_FOCUS_JS = """(i) => {
  const bs = Array.from(document.querySelectorAll('button:not([disabled])')).filter((b) => { const r = b.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(b).visibility !== 'hidden'; });
  const b = bs[i]; if (!b) return null;
  b.scrollIntoView({ block: 'center' }); b.focus(); window.__kb = 0; b.addEventListener('click', () => { window.__kb++; });
  return { label: (b.id ? '#' + b.id : (b.textContent || '').trim().slice(0, 16)), focused: document.activeElement === b };
}"""

KEYBOARD_COUNT_JS = """() => Array.from(document.querySelectorAll('button:not([disabled])')).filter((b) => { const r = b.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(b).visibility !== 'hidden'; }).length"""

KEYBOARD_BUTTONS_PER_STATE = 4
CONFIRM_RE = re.compile(r"\bconfirm\s*\(")


def load_types(path: Path) -> dict[str, dict]:
    """page-types.toml を読む。キーは閉じていて、検査の種別と名前は閉語彙。型の id → {title, requirements} を返す。"""
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise SpecError(f"{path}: 読めません({e})") from e
    extra = set(data) - {"type"}
    if extra:
        raise SpecError(f"{path}: 知らないキー {sorted(extra)}")
    out: dict[str, dict] = {}
    seen_reqs: set[str] = set()
    for t in data.get("type") or []:
        extra = set(t) - {"id", "title", "requirement"}
        if extra:
            raise SpecError(f"{path}: type に知らないキー {sorted(extra)}")
        if not NAME_RE.match(str(t.get("id", ""))) or not isinstance(t.get("title"), str) or not t["title"]:
            raise SpecError(f"{path}: type には id([a-z0-9-]+)と title が要ります: {t.get('id')!r}")
        if t["id"] in out:
            raise SpecError(f"{path}: type.id が重複しています: {t['id']}")
        reqs = []
        for r in t.get("requirement") or []:
            extra = set(r) - {"id", "text", "check", "audit"}
            if extra:
                raise SpecError(f"{path}: requirement に知らないキー {sorted(extra)}")
            if not REQ_ID_RE.match(str(r.get("id", ""))):
                raise SpecError(f"{path}: requirement.id の形が違います: {r.get('id')!r}")
            if r["id"] in seen_reqs:
                raise SpecError(f"{path}: requirement.id が重複しています: {r['id']}")
            seen_reqs.add(r["id"])
            if not isinstance(r.get("text"), str) or not r["text"]:
                raise SpecError(f"{path}: {r['id']} に text がありません")
            if r.get("check") not in CHECKS:
                raise SpecError(f"{path}: {r['id']} の check は {CHECKS} のどれか: {r.get('check')!r}")
            if (r["check"] == "audit") != ("audit" in r):
                raise SpecError(f"{path}: {r['id']}: audit の名前は、check = \"audit\" の要件だけが持ちます")
            if "audit" in r and r["audit"] not in AUDITS:
                raise SpecError(f"{path}: {r['id']} の audit は {AUDITS} のどれか: {r['audit']!r}")
            reqs.append(r)
        out[t["id"]] = {"title": t["title"], "requirements": reqs}
    if "common" not in out:
        raise SpecError(f"{path}: 型 common がありません")
    audited = {r["audit"] for t in out.values() for r in t["requirements"] if "audit" in r}
    unused = set(AUDITS) - audited
    if unused:
        raise SpecError(f"{path}: どの要件にも結びついていない検査があります: {sorted(unused)}")
    return out


def cell_findings(cell: dict, host: str, audits: set[str]) -> list[tuple[str, str]]:
    """撮った 1 セルの測定から、(検査の名前, 違反の説明) を作る。測定だけを見る純粋関数(ブラウザ無しで試験できる)。"""
    m = cell.get("measures") or {}
    out: list[tuple[str, str]] = []

    def add(audit: str, detail: str) -> None:
        if audit in audits:
            out.append((audit, detail))

    if host != "artifact":  # Artifact の断片は、ホストの殻が lang と viewport を与える
        if not m.get("lang"):
            add("document", "`<html lang>` が空")
        if not m.get("viewport"):
            add("document", "viewport の `<meta>` が無い")
    if not m.get("title"):
        add("document", "`<title>` が空")
    if m.get("mains") != 1:
        add("document", f"`<main>` が {m.get('mains')} 個(1 個だけ)")
    if not m.get("h1s"):
        add("document", "`<h1>` が無い")
    if m.get("contrastFailCount"):
        worst = min(m["contrastFails"], key=lambda f: f["ratio"])
        add("contrast", f"基準に届かない文字が {m['contrastFailCount']} 件。最悪は {worst['el']}「{worst['text']}」の {worst['ratio']}:1({worst['px']:g}px)")
    n_small = m.get("smallControlCount", 0) + m.get("smallLinkCount", 0)
    if n_small:
        first = (m.get("smallControls") or m.get("smallLinks") or [{}])[0]
        add("targets", f"24px 未満の操作部が {n_small} 件(例: {first.get('el')} {first.get('w')}×{first.get('h')})")
    if m.get("smallLinkCount"):
        first = m["smallLinks"][0]
        add("links", f"24px 未満のリンクが {m['smallLinkCount']} 件(例: {first['el']} {first['w']}×{first['h']})")
    if cell.get("width") == 390 and cell.get("overflow_x"):
        add("overflow", "幅 390px で横スクロールが出る")
    if cell.get("blocked"):
        add("third-party", f"第三者への要求 {len(cell['blocked'])} 件(例: {cell['blocked'][0]})")
    if m.get("mainButtons", 0) > 1:
        add("main-button", f"主操作(`.btn-main`)が {m['mainButtons']} 個")
    if cell.get("state") == "initial" and m.get("checkedRadios"):
        add("default-choice", f"初期状態で選択済みのラジオが {m['checkedRadios']} 個")
    if "confirm" in (cell.get("dialogs") or []):
        add("confirm", "確認ダイアログ(`confirm`)が開いた")
    return out


def _keyboard_findings(browser, site: Path, p: dict, s: dict, stub: str) -> list[str]:
    """ボタンにフォーカスして Enter・Space を押し、ボタンが押されるかを見る(ショートカットが標準のキー操作を奪わないか)。
    違反の説明は状態を含まない(状態は、違反を出した場所として別に持つ)。"""
    out: list[str] = []
    ctx, page, *_ = _open(browser, site, p, s, "light", 1280, stub)
    try:
        n = min(int(page.evaluate(KEYBOARD_COUNT_JS)), KEYBOARD_BUTTONS_PER_STATE)
    finally:
        ctx.close()
    for i in range(n):
        for key in ("Enter", "Space"):
            ctx, page, *_ = _open(browser, site, p, s, "light", 1280, stub)
            try:
                info = page.evaluate(KEYBOARD_FOCUS_JS, i)
                if not info or not info["focused"]:
                    continue
                page.keyboard.press(key)
                page.wait_for_timeout(100)
                if page.evaluate("window.__kb") == 0:
                    out.append(f"ボタン「{info['label']}」にフォーカスして {key} を押しても、押されない")
            finally:
                ctx.close()
    return out


def run_audit(spec_path: Path, types_path: Path, ref: str, only: str | None = None) -> dict:
    types = load_types(types_path)
    pages = {p["id"]: p for p in load_spec(spec_path)}
    for p in pages.values():
        unknown = [t for t in p.get("types", ["common"]) if t not in types]
        if unknown:
            raise SpecError(f"{p['id']}: types に、{types_path.name} に無い型 {unknown}")
    report: dict = {"ref": ref, "pages": []}
    cur: dict | None = None
    cells_by_page: dict[str, list] = {}
    keyboard_by_page: dict[str, list[tuple[str, str]]] = {}
    texts_by_page: dict[str, str] = {}
    for browser, site, p, s, stub in _walk(spec_path, ref, only):
        reqs = [r for t in p.get("types", ["common"]) for r in types[t]["requirements"]]
        audits = {r["audit"] for r in reqs if r["check"] == "audit"}
        texts_by_page[p["id"]] = (site / p["entry"]).read_text(encoding="utf-8", errors="replace")
        with tempfile.TemporaryDirectory(prefix="visual-audit-") as shots:
            for theme in THEMES:
                for width in WIDTHS:
                    cell = _shoot(browser, site, p, s, theme, width, stub, Path(shots), measure=True)
                    cells_by_page.setdefault(p["id"], []).append((cell, audits))
        if "keyboard" in audits:
            keyboard_by_page.setdefault(p["id"], []).extend(
                (detail, s["name"]) for detail in _keyboard_findings(browser, site, p, s, stub)
            )
    for pid, cells in cells_by_page.items():
        p = pages[pid]
        reqs = [r for t in p.get("types", ["common"]) for r in types[t]["requirements"]]
        by_audit = {r["audit"]: r for r in reqs if r["check"] == "audit"}
        found: dict[str, dict[str, list[str]]] = {}
        for cell, audits in cells:
            for audit, detail in cell_findings(cell, p.get("host", "site"), audits):
                found.setdefault(audit, {}).setdefault(detail, []).append(cell["cell"])
        if "confirm" in by_audit and CONFIRM_RE.search(texts_by_page[pid]):
            found.setdefault("confirm", {}).setdefault("ページのスクリプトが `confirm(` を呼ぶ", []).append("(静的)")
        for detail, state in keyboard_by_page.get(pid, []):
            found.setdefault("keyboard", {}).setdefault(detail, []).append(f"状態 {state}")
        violations = [
            {"req": by_audit[a]["id"], "text": by_audit[a]["text"], "detail": d, "cells": c}
            for a, details in found.items() for d, c in details.items()
        ]
        violations.sort(key=lambda v: (v["req"], v["detail"]))
        passed = sorted(r["id"] for r in by_audit.values() if r["audit"] not in found)
        manual = [{"req": r["id"], "check": r["check"], "text": r["text"]} for r in reqs if r["check"] != "audit"]
        report["pages"].append({"page": pid, "types": p.get("types", ["common"]), "violations": violations, "passed": passed, "manual": manual})
    return report


def summarize_audit(report: dict) -> str:
    lines: list[str] = []
    for pg in report["pages"]:
        lines.append(f"[{pg['page']}] 型 {', '.join(pg['types'])}: 違反 {len(pg['violations'])} 件 / 通った検査 {len(pg['passed'])} 件({' '.join(pg['passed'])}) / 人が見る {len(pg['manual'])} 件")
        for v in pg["violations"]:
            where = "、".join(v["cells"][:3]) + (f" ほか {len(v['cells']) - 3} セル" if len(v["cells"]) > 3 else "")
            lines.append(f"  違反 {v['req']}: {v['detail']}  [{where}]")
        manual = " ".join(f"{m['req']}({'WT' if m['check'] == 'walkthrough' else '目視'})" for m in pg["manual"])
        lines.append(f"  未検査(機械では測れない。人が見る): {manual}")
    return "\n".join(lines)


# ── 差分 ──────────────────────────────────────────────────────────────────────

def compare(before, after, tol: int = 8) -> dict:
    """2 枚の画像を比べる。高さ・幅が違うときは、短い側を PAD_FILL で埋めて比べる(差として数える)。"""
    from PIL import Image, ImageChops

    b = before.convert("RGB")
    a = after.convert("RGB")
    w, h = max(b.width, a.width), max(b.height, a.height)

    def pad(img):
        if img.size == (w, h):
            return img
        canvas = Image.new("RGB", (w, h), PAD_FILL)
        canvas.paste(img, (0, 0))
        return canvas

    b2, a2 = pad(b), pad(a)
    r, g, bl = ImageChops.difference(b2, a2).split()
    m = ImageChops.lighter(ImageChops.lighter(r, g), bl).point(lambda v: 255 if v > tol else 0)
    count = m.histogram()[255]
    bbox = m.getbbox()
    return {
        "status": "same" if count == 0 else "changed",
        "changed_px": count,
        "ratio": round(count / (w * h), 6),
        "bbox": list(bbox) if bbox else None,
        "before_size": list(b.size),
        "after_size": list(a.size),
        "_images": (b2, a2, m),
    }


def triptych(b2, a2, mask):
    from PIL import Image, ImageDraw, ImageFilter, ImageFont

    w, h = a2.size
    dim = Image.blend(a2, Image.new("RGB", (w, h), "white"), 0.7)
    red = Image.new("RGB", (w, h), (230, 0, 40))
    diff_img = Image.composite(red, dim, mask.filter(ImageFilter.MaxFilter(5)))
    label_h, gap = 30, 16
    sheet = Image.new("RGB", (w * 3 + gap * 4, h + label_h + gap * 2), (238, 238, 238))
    font = ImageFont.load_default(size=18)
    draw = ImageDraw.Draw(sheet)
    for i, (label, img) in enumerate((("Before", b2), ("After", a2), ("Diff", diff_img))):
        x = gap + i * (w + gap)
        draw.text((x, gap // 2), label, fill=(30, 30, 30), font=font)
        sheet.paste(img, (x, label_h + gap))
        draw.rectangle((x - 1, label_h + gap - 1, x + w, label_h + gap + h), outline=(120, 120, 120))
    limit = 3600
    if sheet.width > limit:
        sheet = sheet.resize((limit, round(sheet.height * limit / sheet.width)), Image.LANCZOS)
    return sheet


CROP_MIN_HEIGHT = 1600  # これより高い画像だけ、変更箇所の切り出しを足す(全体では変更箇所が小さくて見えない)
CROP_MAX_BANDS = 3
CROP_MAX_HEIGHT = 1400
CROP_MARGIN = 40
CROP_MIN_WIDTH = 480


def change_bands(mask, gap: int = 120, chunk: int = 20) -> list[tuple[int, int]]:
    """変化した行を、近いものどうしでまとめた縦の区間([上, 下))にする。"""
    w, h = mask.size
    rows = [mask.crop((0, y, w, min(y + chunk, h))).getbbox() is not None for y in range(0, h, chunk)]
    bands: list[list[int]] = []
    for i, on in enumerate(rows):
        if not on:
            continue
        if bands and (i - bands[-1][1] - 1) * chunk <= gap:
            bands[-1][1] = i
        else:
            bands.append([i, i])
    return [(s * chunk, min((e + 1) * chunk, h)) for s, e in bands]


def diff_dirs(before: Path, after: Path, out: Path, tol: int = 8) -> dict:
    from PIL import Image

    out.mkdir(parents=True, exist_ok=True)
    names_b = {p.stem for p in before.glob("*.png")}
    names_a = {p.stem for p in after.glob("*.png")}
    cells = []
    for name in sorted(names_b | names_a):
        if name not in names_a:
            cells.append({"cell": name, "status": "removed"})
        elif name not in names_b:
            # 新規のセルは、After の画像をそのまま出す(Before が無いので、並べる相手が無い)
            shutil.copyfile(after / f"{name}.png", out / f"{name}.png")
            with Image.open(after / f"{name}.png") as ai:
                cells.append({"cell": name, "status": "added", "after_size": list(ai.size)})
        else:
            with Image.open(before / f"{name}.png") as bi, Image.open(after / f"{name}.png") as ai:
                res = compare(bi, ai, tol)
                b2, a2, mask = res.pop("_images")
                if res["status"] == "changed":
                    triptych(b2, a2, mask).save(out / f"{name}.png")
                    res["crops"] = []
                    if b2.height > CROP_MIN_HEIGHT:
                        w, h = b2.size
                        for n, (top, bottom) in enumerate(change_bands(mask)[:CROP_MAX_BANDS], 1):
                            bb = mask.crop((0, top, w, bottom)).getbbox()
                            x0, x1 = max(0, bb[0] - CROP_MARGIN), min(w, bb[2] + CROP_MARGIN)
                            if x1 - x0 < CROP_MIN_WIDTH:  # 細い変更は、読める幅まで左右に広げる
                                mid = (x0 + x1) // 2
                                x0, x1 = max(0, mid - CROP_MIN_WIDTH // 2), min(w, mid + CROP_MIN_WIDTH // 2)
                            box = (x0, max(0, top - CROP_MARGIN), x1, min(h, min(bottom + CROP_MARGIN, top + CROP_MAX_HEIGHT)))
                            fn = f"{name}.crop{n}.png"
                            triptych(b2.crop(box), a2.crop(box), mask.crop(box)).save(out / fn)
                            res["crops"].append({"file": fn, "top": box[1], "bottom": box[3], "left": box[0], "right": box[2]})
            cells.append({"cell": name, **res})
    report = {"tolerance": tol, "cells": cells}
    (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def summarize(report: dict) -> str:
    cs = report["cells"]
    n = {k: sum(1 for c in cs if c["status"] == k) for k in ("changed", "same", "added", "removed")}
    lines = [f"セル {len(cs)} 件: 差あり {n['changed']} / 差なし {n['same']} / 新規 {n['added']} / 削除 {n['removed']}"]
    for c in cs:
        if c["status"] == "changed":
            lines.append(f"  差あり {c['cell']}: {c['changed_px']} px({c['ratio'] * 100:.2f}%) 範囲 {c['bbox']} 高さ {c['before_size'][1]}→{c['after_size'][1]}")
        elif c["status"] in ("added", "removed"):
            lines.append(f"  {'新規' if c['status'] == 'added' else '削除'} {c['cell']}")
    return "\n".join(lines)


# ── 並べたページ ──────────────────────────────────────────────────────────────

PAGE_CSS = """
:root { color-scheme: light dark; --bg:#fbfbfa; --fg:#1b1d1f; --muted:#5b6066; --line:#d6d8da; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { --bg:#16181a; --fg:#eceeef; --muted:#a3a8ad; --line:#3a3e42; } }
:root[data-theme="dark"] { --bg:#16181a; --fg:#eceeef; --muted:#a3a8ad; --line:#3a3e42; color-scheme: dark; }
:root[data-theme="light"] { color-scheme: light; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.7 system-ui, "Noto Sans JP", sans-serif; }
main { max-width: 72rem; margin: 0 auto; padding: 1.5rem 1rem 4rem; }
h1 { font-size: 1.5rem; } h2 { font-size: 1.1rem; margin-top: 2.5rem; } .meta { color: var(--muted); font-size: .9rem; }
.cell { border-top: 1px solid var(--line); padding-top: 1rem; margin-top: 1.5rem; }
.cell img { max-width: 100%; height: auto; display: block; border: 1px solid var(--line); }
table { border-collapse: collapse; width: 100%; } th, td { text-align: left; padding: .4rem .6rem; border-bottom: 1px solid var(--line); }
"""


def build_page(report: dict, title: str, urls: dict[str, str] | None = None, ref_note: str = "") -> str:
    urls = urls or {}
    e = html.escape
    cs = report["cells"]
    changed = [c for c in cs if c["status"] == "changed"]
    added = [c for c in cs if c["status"] == "added"]
    removed = [c for c in cs if c["status"] == "removed"]
    same = [c for c in cs if c["status"] == "same"]
    parts = [
        f"<h1>{e(title)}</h1>",
        f'<p class="meta">セル {len(cs)} 件(面 × 状態 × 明暗 × 幅): 差あり {len(changed)} / 差なし {len(same)} / 新規 {len(added)} / 削除 {len(removed)}'
        f'。許容差 {report.get("tolerance", 0)}/255。{e(ref_note)}</p>',
    ]
    if changed:
        parts.append("<h2>差があるセル</h2>")
        for c in changed:
            fn = f"{c['cell']}.png"
            crops = "".join(
                f'<figure><figcaption class="meta">変更箇所 {n}(上から {cr["top"]}〜{cr["bottom"]} px)</figcaption>'
                f'<img src="{e(urls.get(cr["file"], cr["file"]))}" alt="{e(c["cell"])} の変更箇所 {n} の Before・After・Diff を横に並べた画像"></figure>'
                for n, cr in enumerate(c.get("crops", []), 1)
            )
            full = (
                f'<img src="{e(urls.get(fn, fn))}" alt="{e(c["cell"])} の全体の Before・After・Diff を横に並べた画像">'
            )
            full = f"<details><summary>ページ全体</summary>{full}</details>" if crops else full
            parts.append(
                f'<section class="cell"><h3>{e(c["cell"])}</h3>'
                f'<p class="meta">{c["changed_px"]} px({c["ratio"] * 100:.2f}%)。範囲 {e(str(c["bbox"]))}。高さ {c["before_size"][1]}→{c["after_size"][1]}</p>'
                f"{crops}{full}</section>"
            )
    if added:
        parts.append("<h2>新規のセル(Before なし。After の画像)</h2>")
        for n, c in enumerate(added):
            fn = f"{c['cell']}.png"
            size = c.get("after_size") or ["?", "?"]
            parts.append(
                f'<details{" open" if n < 2 else ""}><summary>{e(c["cell"])}({size[0]}×{size[1]})</summary>'
                f'<img src="{e(urls.get(fn, fn))}" alt="{e(c["cell"])} の After の画像"></details>'
            )
    if removed:
        parts.append("<h2>削除されたセル(After なし)</h2><ul>" + "".join(f"<li>{e(c['cell'])}</li>" for c in removed) + "</ul>")
    if same:
        parts.append("<h2>差なしのセル</h2><table><thead><tr><th>セル</th><th>大きさ</th></tr></thead><tbody>")
        parts.extend(f"<tr><td>{e(c['cell'])}</td><td>{c['after_size'][0]}×{c['after_size'][1]}</td></tr>" for c in same)
        parts.append("</tbody></table>")
    body = "\n".join(parts)
    return (
        '<!doctype html><html lang="ja"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        f"<title>{e(title)}</title><style>{PAGE_CSS}</style></head><body><main>{body}</main></body></html>"
    )


# ── 自己検査 ──────────────────────────────────────────────────────────────────

def selftest(browser: bool) -> int:
    from PIL import Image

    fails: list[str] = []

    def check(cond: bool, msg: str) -> None:
        print(("  ok  " if cond else "  NG  ") + msg)
        if not cond:
            fails.append(msg)

    def solid(w, h, rgb=(250, 250, 250)):
        return Image.new("RGB", (w, h), rgb)

    # 画素の差
    base = solid(40, 30)
    r = compare(base, base.copy())
    check(r["status"] == "same" and r["changed_px"] == 0 and r["bbox"] is None, "同じ画像は差なし")
    px = base.copy()
    px.putpixel((5, 7), (0, 0, 0))
    r = compare(base, px)
    check(r["status"] == "changed" and r["changed_px"] == 1 and r["bbox"] == [5, 7, 6, 8], "1 画素の違いを、その位置で拾う")
    near = base.copy()
    near.putpixel((5, 7), (245, 250, 250))
    check(compare(base, near, tol=8)["status"] == "same", "許容差の内側の違い(5/255)は差なし")
    check(compare(base, near, tol=2)["status"] == "changed", "許容差を絞れば同じ違いを拾う")
    tall = solid(40, 50)
    r = compare(base, tall)
    check(r["status"] == "changed" and r["before_size"] == [40, 30] and r["after_size"] == [40, 50], "高さが違えば、同じ色でも差あり(埋めた分を数える)")
    check(compare(base, solid(60, 30))["status"] == "changed", "幅が違えば差あり")

    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        # ディレクトリの比較
        (t / "b").mkdir()
        (t / "a").mkdir()
        base.save(t / "b/p--s--light--390.png")
        base.save(t / "a/p--s--light--390.png")
        base.save(t / "b/p--s--dark--390.png")
        px.save(t / "a/p--s--dark--390.png")
        base.save(t / "b/gone--s--light--390.png")
        base.save(t / "a/new--s--light--390.png")
        rep = diff_dirs(t / "b", t / "a", t / "d")
        st = {c["cell"]: c["status"] for c in rep["cells"]}
        check(
            st == {"p--s--light--390": "same", "p--s--dark--390": "changed", "gone--s--light--390": "removed", "new--s--light--390": "added"},
            "セルごとに 差なし・差あり・削除・新規 を分ける",
        )
        check((t / "d/p--s--dark--390.png").is_file() and not (t / "d/p--s--light--390.png").exists(), "差があるセルだけ並べた画像を出す")
        check((t / "d/report.json").is_file(), "report.json を出す")
        check((t / "d/new--s--light--390.png").is_file(), "新規のセルは、After の画像をそのまま出す")
        check(not any(p.name.endswith(".crop1.png") for p in (t / "d").iterdir()), "低い画像には、変更箇所の切り出しを足さない")
        # 高い画像: 離れた 2 か所の変更を、2 つの切り出しにする
        tallb, tallb2 = solid(100, 3000), solid(100, 3000)
        tallb2.putpixel((10, 200), (0, 0, 0))
        tallb2.putpixel((10, 2500), (0, 0, 0))
        (t / "tb").mkdir()
        (t / "ta").mkdir()
        tallb.save(t / "tb/q--s--light--390.png")
        tallb2.save(t / "ta/q--s--light--390.png")
        rep_t = diff_dirs(t / "tb", t / "ta", t / "td")
        crops = rep_t["cells"][0]["crops"]
        check(len(crops) == 2 and crops[0]["top"] <= 200 < crops[0]["bottom"] and crops[1]["top"] <= 2500 < crops[1]["bottom"], "高い画像の離れた 2 か所の変更を、2 つの切り出しにする")
        check(all((t / "td" / cr["file"]).is_file() for cr in crops), "切り出しの画像を出す")
        pg_t = build_page(rep_t, "t")
        check("変更箇所 1" in pg_t and "変更箇所 2" in pg_t and "<details>" in pg_t, "ページは切り出しを先に、全体を折りたたみで出す")
        # ページ
        pg = build_page(rep, "題 <b>", {"p--s--dark--390.png": "https://example.invalid/x.png"})
        check('src="https://example.invalid/x.png"' in pg and "題 &lt;b&gt;" in pg, "並べたページは、画像の URL を差し替え、題を escape する")
        check("差なしのセル" in pg and "p--s--light--390" in pg and "新規のセル" in pg and "削除されたセル" in pg, "差なし・新規・削除を表と一覧に分ける")
        loads_third_party = "https://" in pg.replace("https://example.invalid/x.png", "")
        check('lang="ja"' in pg and "<main>" in pg and "<h1>" in pg and not loads_third_party, "ページは lang・main・h1 を持ち、第三者を読み込まない")

        # 撮影定義
        good = t / "ok.toml"
        good.write_text('[[page]]\nid = "a"\nroot = "d"\nentry = "i.html"\n[[page.state]]\nname = "s1"\nsteps = [ { click = "#x" } ]\n', encoding="utf-8")
        check(load_spec(good)[0]["state"][0]["name"] == "s1", "正しい撮影定義を読む")
        for label, body in (
            ("知らないキー", '[[page]]\nid = "a"\nroot = "d"\nentry = "i.html"\nbogus = 1\n'),
            ("root が上位へ出る", '[[page]]\nid = "a"\nroot = "../d"\nentry = "i.html"\n'),
            ("root が根そのもの", '[[page]]\nid = "a"\nroot = "."\nentry = "i.html"\n'),
            ("id の形", '[[page]]\nid = "A_b"\nroot = "d"\nentry = "i.html"\n'),
            ("step が 2 種", '[[page]]\nid = "a"\nroot = "d"\nentry = "i.html"\n[[page.state]]\nname = "s"\nsteps = [ { click = "#x", press = "Tab" } ]\n'),
            ("fill に value が無い", '[[page]]\nid = "a"\nroot = "d"\nentry = "i.html"\n[[page.state]]\nname = "s"\nsteps = [ { fill = "#x" } ]\n'),
        ):
            bad = t / "bad.toml"
            bad.write_text(body, encoding="utf-8")
            try:
                load_spec(bad)
                check(False, f"撮影定義の誤りを落とす: {label}")
            except SpecError:
                check(True, f"撮影定義の誤りを落とす: {label}")

        # ページの型の要件(page-types.toml)
        real_types = Path(__file__).resolve().parents[5] / "config/web/page-types.toml"
        if real_types.is_file():
            ty = load_types(real_types)
            reqs = [r for v in ty.values() for r in v["requirements"]]
            check(set(ty) == {"common", "study", "form", "dashboard"} and len(reqs) == 38, "実物の page-types.toml を読める(4 つの型・38 の要件)")
            check({r["audit"] for r in reqs if "audit" in r} == set(AUDITS), "すべての検査が、どれかの要件に結びついている")
            check(all(r["check"] == "audit" for r in reqs if r["id"] in ("C1", "C2", "C3", "C4", "C9", "C12", "F1", "F2", "F4", "D7")), "機械で測れると宣言した要件は、audit")
        for label, body in (
            ("知らないキー", '[[type]]\nid = "common"\ntitle = "c"\nbogus = 1\n'),
            ("check が語彙の外", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "maybe"\n'),
            ("walkthrough に audit の名前", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "walkthrough"\naudit = "document"\n'),
            ("audit の名前が語彙の外", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "audit"\naudit = "magic"\n'),
            ("audit に名前が無い", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "audit"\n'),
            ("要件の id が重複", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "visual"\n[[type.requirement]]\nid = "C1"\ntext = "y"\ncheck = "visual"\n'),
            ("要件の id の形", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "X1"\ntext = "x"\ncheck = "visual"\n'),
            ("型 common が無い", '[[type]]\nid = "form"\ntitle = "f"\n'),
            ("どの要件にも結びつかない検査がある", '[[type]]\nid = "common"\ntitle = "c"\n[[type.requirement]]\nid = "C1"\ntext = "x"\ncheck = "visual"\n'),
        ):
            bad = t / "bad-types.toml"
            bad.write_text(body, encoding="utf-8")
            try:
                load_types(bad)
                check(False, f"型の要件の誤りを落とす: {label}")
            except SpecError:
                check(True, f"型の要件の誤りを落とす: {label}")
        good_cell = {
            "cell": "p--initial--light--390", "state": "initial", "width": 390, "overflow_x": False, "blocked": [], "dialogs": [],
            "measures": {"lang": "ja", "title": "t", "viewport": True, "mains": 1, "h1s": 1, "contrastFailCount": 0, "contrastFails": [],
                         "smallControlCount": 0, "smallControls": [], "smallLinkCount": 0, "smallLinks": [], "mainButtons": 1, "checkedRadios": 0},
        }
        every = set(AUDITS)
        check(cell_findings(good_cell, "site", every) == [], "測定が基準を満たすセルは、違反なし")
        bad_cell = {
            **good_cell, "overflow_x": True, "blocked": ["https://x.example/a.css"], "dialogs": ["confirm"],
            "measures": {**good_cell["measures"], "lang": "", "title": "", "viewport": False, "mains": 0, "h1s": 0, "contrastFailCount": 2,
                         "contrastFails": [{"el": "p", "text": "薄い", "ratio": 2.1, "px": 16}], "smallControlCount": 1,
                         "smallControls": [{"el": "button#x", "w": 10, "h": 10}], "smallLinkCount": 1, "smallLinks": [{"el": "a", "w": 40, "h": 12}],
                         "mainButtons": 2, "checkedRadios": 1},
        }
        got = {a for a, _ in cell_findings(bad_cell, "site", every)}
        check(got == every - {"keyboard"}, f"基準を外れた測定は、keyboard 以外のすべての検査が違反を出す({sorted(every - {'keyboard'} - got)} が漏れ)")
        check(cell_findings(bad_cell, "site", {"contrast"}) == [("contrast", cell_findings(bad_cell, "site", {"contrast"})[0][1])] and len(cell_findings(bad_cell, "site", {"contrast"})) == 1, "面の型が持たない検査は、違反に数えない")
        art = {a for a, _ in cell_findings({**bad_cell, "width": 1280}, "artifact", {"document", "overflow"})}
        check("overflow" not in art, "横スクロールは、幅 390px のセルだけで見る")
        doc = [d for a, d in cell_findings(bad_cell, "artifact", {"document"})]
        check(not any("lang" in d or "viewport" in d for d in doc), "Artifact の断片は、lang と viewport を求めない(ホストの殻が与える)")
        check(any("<main>" in d for d in doc) and any("<h1>" in d for d in doc), "Artifact でも、main と h1 は求める")
        rc = {**good_cell, "state": "after-pick"}
        rc["measures"] = {**good_cell["measures"], "checkedRadios": 1}
        check(cell_findings(rc, "site", {"default-choice"}) == [], "選択済みのラジオは、初期状態だけで違反にする")
        check(CONFIRM_RE.search("if (confirm('x')) go()") is not None and CONFIRM_RE.search("function confirmAll() {}") is None, "`confirm(` の静的な検出は、confirmAll などを拾わない")

        # Before は ref から取り、作業ツリーからは取れない
        repo = t / "repo"
        (repo / "site").mkdir(parents=True)
        env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}

        def git(*args):
            subprocess.run(
                ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", *args],
                check=True, capture_output=True, env=env,
            )

        git("init", "-q", "-b", "main")
        (repo / "site/i.html").write_text("v1", encoding="utf-8")
        git("add", ".")
        git("commit", "-q", "-m", "v1")
        (repo / "site/i.html").write_text("v2", encoding="utf-8")
        materialize(repo, "site", "HEAD", t / "m-head")
        materialize(repo, "site", "WORKTREE", t / "m-work")
        check((t / "m-head/i.html").read_text() == "v1", "Before: ref から取り出した内容は、編集前(v1)")
        check((t / "m-work/i.html").read_text() == "v2", "After: WORKTREE は、編集後(v2)")

        if browser:
            (repo / "site/i.html").write_text(
                '<!doctype html><meta charset=utf-8><title>t</title><style>body{margin:0;background:#fff;color:#000}'
                '@media (prefers-color-scheme: dark){body{background:#000;color:#fff}}</style><div style="height:300px">x</div>',
                encoding="utf-8",
            )
            (repo / "visual.toml").write_text('[[page]]\nid = "t"\nroot = "site"\nentry = "i.html"\n', encoding="utf-8")
            git("add", ".")
            git("commit", "-q", "-m", "v3")
            m1 = capture(repo / "visual.toml", "HEAD", t / "c1")
            capture(repo / "visual.toml", "WORKTREE", t / "c2")
            check(len(m1["cells"]) == len(THEMES) * len(WIDTHS), "撮る行列は 明暗 × 幅")
            rep2 = diff_dirs(t / "c1", t / "c2", t / "d2")
            check(all(c["status"] == "same" for c in rep2["cells"]), "同じ入力で 2 回撮った画像は、画素単位で一致する(決定的)")
            light = Image.open(t / "c1/t--initial--light--390.png")
            dark = Image.open(t / "c1/t--initial--dark--390.png")
            check(compare(light, dark)["status"] == "changed", "明と暗が実際に描き分けられる")
            check(light.width == 390 and Image.open(t / "c1/t--initial--light--1280.png").width == 1280, "幅は要求どおり(Firefox の 0.75 倍にならない)")

            # 型の要件の検査: わざと悪いページ(番兵)が、すべての検査で落ち、良いページが通る
            ids = {"C1": "document", "C2": "contrast", "C3": "targets", "C4": "overflow", "C9": "third-party",
                   "C12": "keyboard", "F1": "main-button", "F2": "default-choice", "F4": "confirm", "D7": "links"}
            toml_types = '[[type]]\nid = "common"\ntitle = "c"\n' + "".join(
                f'[[type.requirement]]\nid = "{i}"\ntext = "{a}"\ncheck = "audit"\naudit = "{a}"\n' for i, a in ids.items())
            (t / "types.toml").write_text(toml_types, encoding="utf-8")
            (repo / "site/bad.html").write_text(
                '<!doctype html><title></title><style>body{margin:0;background:#fff;color:#bbb}.wide{width:900px;height:10px;background:#000}</style>'
                '<p>薄い文字</p><button id="tiny" style="width:10px;height:10px">x</button>'
                '<button class="btn-main">a</button><button class="btn-main">b</button><input type="radio" name="r" checked>'
                '<a href="#" style="display:block;height:10px">l</a><div class="wide"></div><img src="https://example.invalid/x.png">'
                '<button id="steal">steal</button>'
                "<script>document.addEventListener('keydown',function(e){if(e.target.id==='steal'&&(e.key==='Enter'||e.key===' '))e.preventDefault();});"
                "if(false){confirm('x');}</script>", encoding="utf-8")
            (repo / "site/good.html").write_text(
                '<!doctype html><html lang="ja"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>t</title>'
                '<main><h1>見出し</h1><p>本文</p><button style="min-height:44px;min-width:44px">ok</button>'
                '<label style="display:block;min-height:44px"><input type="radio" name="q">ラベルで囲まれた小さいラジオ(行全体が押せる)</label></main>', encoding="utf-8")
            (repo / "visual.toml").write_text(
                '[[page]]\nid = "bad"\nroot = "site"\nentry = "bad.html"\n[[page]]\nid = "good"\nroot = "site"\nentry = "good.html"\n', encoding="utf-8")
            rep = run_audit(repo / "visual.toml", t / "types.toml", "WORKTREE")
            by = {pg["page"]: pg for pg in rep["pages"]}
            got = {v["req"] for v in by["bad"]["violations"]}
            check(got == set(ids), f"悪いページは、10 の検査のすべてで落ちる(漏れ: {sorted(set(ids) - got)})")
            check(by["good"]["violations"] == [] and sorted(by["good"]["passed"]) == sorted(ids), "良いページは、10 の検査のすべてを通る")
            kb = [v for v in by["bad"]["violations"] if v["req"] == "C12"]
            keys = sorted(k for v in kb for k in ("Enter", "Space") if k in v["detail"])
            check(kb and all("steal" in v["detail"] for v in kb) and keys == ["Enter", "Space"], f"キーを奪うボタンだけを、キーの名前つきで拾う(他のボタンは拾わない): {[v['detail'] for v in kb]}")

    print("OK" if not fails else f"NG: {len(fails)} 件")
    return 1 if fails else 0


# ── CLI ───────────────────────────────────────────────────────────────────────

def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="visual_diff.py", description=__doc__.split("\n")[0])
    ap.add_argument("--selftest", action="store_true", help="自己検査(ブラウザ無しで走る)")
    ap.add_argument("--browser", action="store_true", help="--selftest に、実際の撮影の検査を足す(Chromium が要る)")
    sub = ap.add_subparsers(dest="cmd")
    c = sub.add_parser("capture")
    c.add_argument("--spec", required=True, type=Path)
    c.add_argument("--ref", required=True, help="画面のファイルを取り出す git の ref。作業ツリーなら WORKTREE")
    c.add_argument("--out", required=True, type=Path)
    c.add_argument("--only", help="面[:状態] に絞る")
    d = sub.add_parser("diff")
    d.add_argument("--before", required=True, type=Path)
    d.add_argument("--after", required=True, type=Path)
    d.add_argument("--out", required=True, type=Path)
    d.add_argument("--tolerance", type=int, default=8, help="画素の違いとして数える閾値(0〜255)。既定 8")
    a = sub.add_parser("audit", help="面が宣言した型の要件のうち、描画した HTML で測れるものを検査する")
    a.add_argument("--spec", required=True, type=Path)
    a.add_argument("--types", type=Path, help="page-types.toml(既定: このスクリプトのあるリポジトリの config/web/page-types.toml)")
    a.add_argument("--ref", default="WORKTREE", help="画面のファイルを取り出す git の ref。既定は作業ツリー")
    a.add_argument("--only", help="面[:状態] に絞る")
    a.add_argument("--json", type=Path, help="結果を JSON で書く先")
    g = sub.add_parser("page")
    g.add_argument("--report", required=True, type=Path)
    g.add_argument("--title", required=True)
    g.add_argument("--out", required=True, type=Path)
    g.add_argument("--urls", type=Path, help='{"<セル>.png": "<アップロード後の URL>"} の JSON')
    g.add_argument("--note", default="")
    args = ap.parse_args(argv)

    if args.selftest:
        return selftest(args.browser)
    try:
        if args.cmd == "capture":
            m = capture(args.spec, args.ref, args.out, args.only)
            print(f"{len(m['cells'])} セルを {args.out} に撮った(ref={args.ref})")
            for cell in m["cells"]:
                notes = []
                if cell["blocked"]:
                    notes.append(f"遮断した第三者への要求 {len(cell['blocked'])} 件")
                if cell["page_errors"]:
                    notes.append(f"ページのエラー {len(cell['page_errors'])} 件")
                if cell["overflow_x"]:
                    notes.append("横スクロールあり")
                if notes:
                    print(f"  {cell['cell']}: " + "、".join(notes))
        elif args.cmd == "diff":
            print(summarize(diff_dirs(args.before, args.after, args.out, args.tolerance)))
        elif args.cmd == "audit":
            types_path = args.types or Path(__file__).resolve().parents[5] / "config/web/page-types.toml"
            report = run_audit(args.spec, types_path, args.ref, args.only)
            print(summarize_audit(report))
            if args.json:
                args.json.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
            return 1 if any(pg["violations"] for pg in report["pages"]) else 0
        elif args.cmd == "page":
            report = json.loads(args.report.read_text(encoding="utf-8"))
            urls = json.loads(args.urls.read_text(encoding="utf-8")) if args.urls else None
            args.out.write_text(build_page(report, args.title, urls, args.note), encoding="utf-8")
            print(f"{args.out} を書いた")
        else:
            ap.print_help()
            return 2
    except SpecError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
