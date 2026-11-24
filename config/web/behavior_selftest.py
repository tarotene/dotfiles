#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright==1.63.0"]  # visual_diff.py と同じ版。上げるときは、両方の --selftest --browser を通す
# ///
"""behavior_selftest.py — 振る舞いの共有部品 `behavior.js` を、実際のブラウザで検査する(ADR-1703 D2 の執行点)。

見本ページ `specimen.html`(`behavior.js` と `specimen.js` を読み込む)を、本番と同じ種類の CSP
(`default-src 'none'; script-src 'self'; style-src 'self'; ...`)つきで開き、次を確かめる。

  saver      保存中 → 書き込みの成功を確かめてから「保存済み」。失敗は、再保存の操作つきで出し、再保存で成功に戻れる。
             続けて呼んだとき、古い結果が表示を書き換えない。`false` を返す・同期に throw する書き込みも失敗として扱う。
  keyguard   ボタンにフォーカスして Enter・Space を押すと、ショートカットでなくボタンが押される。フォーカスが無ければ
             ショートカットが呼ばれる。修飾キーつき・文字入力欄の中のキーは、ショートカットが奪わない。
  viewstate  失敗の出し先が最初から DOM にある。読み込み中・中身・空・失敗が、同時には出ない。失敗に再読み込みの操作を出す。
  CSP        上の操作のあいだ、CSP 違反とページのエラーが 0 件。

実行: `uv run --script config/web/behavior_selftest.py`(Chromium が要る。CI では走らない。`visual_diff.py --selftest --browser` と同じ扱い)
"""
from __future__ import annotations

import mimetypes
import sys
from pathlib import Path
from urllib.parse import unquote, urlparse

HOST = "http://behavior.test"
CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'"
ROOT = Path(__file__).resolve().parent


def serve(route) -> None:
    url = route.request.url
    if not url.startswith(HOST + "/"):
        route.abort()
        return
    f = (ROOT / unquote(urlparse(url).path.lstrip("/"))).resolve()
    if f.is_file() and ROOT in f.parents:
        ctype = mimetypes.guess_type(f.name)[0] or "application/octet-stream"
        route.fulfill(
            body=f.read_bytes(),
            headers={"Content-Type": ctype + ("; charset=utf-8" if ctype.startswith(("text/", "application/javascript")) else ""),
                     "Content-Security-Policy": CSP},
        )
    else:
        route.fulfill(status=404, body="")


def main() -> int:
    from playwright.sync_api import sync_playwright

    fails: list[str] = []

    def check(cond: bool, msg: str) -> None:
        print(("  ok  " if cond else "  NG  ") + msg)
        if not cond:
            fails.append(msg)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(chromium_sandbox=False)
        ctx = browser.new_context(viewport={"width": 1280, "height": 900}, locale="ja-JP")
        ctx.route("**/*", serve)
        page = ctx.new_page()
        errors: list[str] = []
        page.on("pageerror", lambda e: errors.append(str(e)[:200]))
        page.add_init_script(
            "window.__csp = []; document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI));"
            "window.__events = []; document.addEventListener('save-failed', () => window.__events.push('save-failed')); "
            "document.addEventListener('saved', () => window.__events.push('saved')); document.addEventListener('view-failed', () => window.__events.push('view-failed'));"
        )
        page.goto(f"{HOST}/specimen.html", wait_until="load")
        check(page.evaluate("typeof window.pageBehavior") == "object", "behavior.js が window.pageBehavior を置く(CSP の下で読み込めた)")
        check(sorted(page.evaluate("Object.keys(window.pageBehavior)")) == ["keyguard", "saver", "viewstate"], "公開する名前は saver・keyguard・viewstate の 3 つだけ")

        def until(expr: str, timeout_ms: int = 3000) -> None:
            # `wait_for_function` は、文字列を eval するので、`script-src 'self'` の CSP の下では使えない(検査する側の都合)。
            waited = 0
            while not page.evaluate(f"() => {expr}"):
                if waited >= timeout_ms:
                    raise AssertionError(f"{timeout_ms}ms 待っても成立しない: {expr}")
                page.wait_for_timeout(50)
                waited += 50

        txt = lambda sel: page.locator(sel).first.inner_text()
        attr = lambda sel, name: page.locator(sel).first.get_attribute(name)
        visible = lambda sel: page.locator(sel).first.is_visible()

        # ── saver ──
        page.click("#demo-save-ok")
        check(attr("#demo-save-status", "data-state") == "saving" and txt("#demo-save-status") == "保存中…", "保存を始めた直後は「保存中…」で、「保存済み」を先に出さない")
        until("document.getElementById('demo-save-status').dataset.state === 'saved'")
        check(txt("#demo-save-status").startswith("保存済み "), "書き込みが成功してから「保存済み(時刻)」になる")
        check(attr("#demo-save-status", "role") == "status", "保存の表示は role=status")
        retry = "#demo-save-status + button.btn-link"
        check(not visible(retry), "成功のとき、再保存の操作は出ない")

        page.click("#demo-save-fail")
        until("document.getElementById('demo-save-status').dataset.state === 'failed'")
        check(txt("#demo-save-status") == "保存できませんでした", "失敗は、その項目の横に、失敗の文で出る")
        check(visible(retry) and txt(retry) == "もう一度保存", "失敗のとき、再保存の操作を一緒に出す")
        check("save-failed" in page.evaluate("window.__events"), "失敗を、save-failed のイベントにも載せる(握りつぶさない)")
        page.click(retry)
        until("document.getElementById('demo-save-status').dataset.state === 'saved'")
        check(not visible(retry), "再保存が成功すると、「保存済み」に戻り、再保存の操作は消える")

        out = page.evaluate(
            """async () => {
              const mk = () => { const s = document.createElement('span'); s.className = 'status'; document.body.appendChild(s); return s; };
              const defer = () => { let res, rej; const p = new Promise((a, b) => { res = a; rej = b; }); return { p, res, rej }; };
              const r = {};
              // 古い保存の失敗が、新しい保存の成功を上書きしない
              const sA = mk(); const dA = defer(), dB = defer(); const calls = [dA, dB];
              const sv = window.pageBehavior.saver(sA, () => calls.shift().p);
              const pA = sv.run('a'); const pB = sv.run('b');
              dB.res(true); await pB; dA.rej(new Error('stale')); const aResult = await pA;
              r.stale = { state: sv.state(), aResult };
              // false を返す書き込みは失敗
              const sF = mk(); const svF = window.pageBehavior.saver(sF, () => false);
              r.falseResult = { ok: await svF.run('x'), state: svF.state() };
              // 同期に throw する書き込みも失敗(例外を外へ出さない)
              const sT = mk(); const svT = window.pageBehavior.saver(sT, () => { throw new Error('sync'); });
              r.syncThrow = { ok: await svT.run('x'), state: svT.state() };
              // 引数の誤りは、すぐ分かる(黙って何もしない形にしない)
              try { window.pageBehavior.saver(null, () => 1); r.badArg = 'no throw'; } catch (e) { r.badArg = e.constructor.name; }
              return r;
            }"""
        )
        check(out["stale"]["state"] == "saved" and out["stale"]["aResult"] is False, f"古い保存の失敗が、新しい保存の成功の表示を上書きしない: {out['stale']}")
        check(out["falseResult"] == {"ok": False, "state": "failed"}, f"`false` を返す書き込みは失敗: {out['falseResult']}")
        check(out["syncThrow"] == {"ok": False, "state": "failed"}, f"同期に throw する書き込みも失敗として画面に出す: {out['syncThrow']}")
        check(out["badArg"] == "TypeError", f"status が要素でないと、すぐ TypeError(黙って何もしない形にしない): {out['badArg']}")

        # ── keyguard ──
        idle = "ショートカットは、まだ呼ばれていない"
        page.focus("#demo-key-button")
        page.keyboard.press("Enter")
        page.keyboard.press("Space")
        check(txt("#demo-key-clicks") == "押された回数: 2", f"ボタンにフォーカスして Enter・Space を押すと、ボタンが 2 回押される: {txt('#demo-key-clicks')}")
        check(txt("#demo-key-out") == idle, "そのあいだ、ショートカットは呼ばれない(標準のキー操作を奪わない)")
        page.evaluate("document.activeElement.blur()")
        page.keyboard.press("1")
        check(txt("#demo-key-out") == "1 のショートカットが呼ばれた", "フォーカスが無ければ、1 はショートカットとして呼ばれる")
        page.keyboard.press("Space")
        check(txt("#demo-key-out") == "Space のショートカットが呼ばれた", "フォーカスが無ければ、Space もショートカットとして呼ばれる")
        page.evaluate("document.getElementById('demo-key-out').textContent = 'x'")
        page.keyboard.press("Control+1")
        check(txt("#demo-key-out") == "x", "Ctrl つきのキーは、ショートカットが奪わない")
        page.evaluate("const t = document.createElement('textarea'); t.id = 'probe'; document.body.appendChild(t); t.focus();")
        page.keyboard.type("1 ")
        check(page.input_value("#probe") == "1 " and txt("#demo-key-out") == "x", "文字入力欄の中の 1・Space は、入力され、ショートカットは呼ばれない")

        # ── viewstate ──
        view = "#demo-view"
        check(page.locator(f"{view} [role=alert]").count() == 1 and not visible(f"{view} [role=alert]"), "失敗の出し先(role=alert)が、最初から DOM にある(失敗するまでは見えない)")
        check(visible(f"{view} .empty") and not visible(f"{view} .status"), "最初は空の表示だけが出る(読み込み中と同時に出ない)")
        page.click("#demo-load-ok")
        check(visible(f"{view} .status") and txt(f"{view} .status") == "読み込み中…" and not visible(f"{view} .empty"), "読み込み中は「読み込み中…」だけが出る")
        check(attr(view, "aria-busy") == "true", "読み込み中は aria-busy=true")
        until("document.querySelectorAll('#demo-view li').length === 2")
        check(page.locator(f"{view} li").count() == 2 and not visible(f"{view} .status") and attr(view, "aria-busy") == "false", "読み込めたら、中身が出て、読み込み中は消える")
        page.click("#demo-load-empty")
        until("!document.querySelector('#demo-view .empty').hidden")
        check(visible(f"{view} .empty") and not visible(f"{view} ul"), "中身が空なら、空の表示になる(中身は出ない)")
        page.click("#demo-load-fail")
        until("!document.querySelector('#demo-view [role=alert]').hidden")
        check("読み込めませんでした" in txt(f"{view} [role=alert]") and visible(f"{view} button.btn-link"), "失敗は、失敗の文と、再読み込みの操作で出る")
        check(not visible(f"{view} .empty") and not visible(f"{view} .status"), "失敗のとき、空・読み込み中は出ない")
        check("view-failed" in page.evaluate("window.__events"), "失敗を、view-failed のイベントにも載せる")
        page.click(f"{view} button.btn-link")
        until("document.querySelectorAll('#demo-view li').length === 2")
        check(not visible(f"{view} [role=alert]"), "再読み込みが成功すると、失敗の表示は消える")

        # ── CSP ──
        check(page.evaluate("window.__csp") == [], f"CSP 違反が 0 件: {page.evaluate('window.__csp')}")
        check(errors == [], f"ページのエラーが 0 件: {errors}")
        browser.close()

    print("OK" if not fails else f"NG: {len(fails)} 件")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
