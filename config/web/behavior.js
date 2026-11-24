/*
 * 振る舞いの共有部品(ADR-1703 D2)。保存の表示・ショートカットのキー・読み込み中/失敗/空の出し分けを、1 か所の実装にする。
 * 面は、このファイルを複写して pin する(ADR-1685。1 枚だけの HTML は、印を付けた区間に複写して pin する: ADR-1700)。
 *
 * - 依存なし。外部の URL を持たない。`import`・`export` を書かない(区間 pin で、インラインの `<script>` に複写できる)。
 * - CSP `style-src 'self'` の下で動く: `.style`・`setAttribute("style")`・`cssText` を書かない。出し入れは `hidden` 属性と
 *   クラス(`components.css` の `.status`・`.alert`・`.empty`・`.btn-link`)で行う。`innerHTML` も使わない(`textContent` だけ)。
 * - 失敗を握りつぶさない: 失敗は画面に出し、`save-failed`・`view-failed` のイベントにも載せる。
 *
 * `window.pageBehavior` に、次の 3 つを置く。
 *   saver(status, write, options)     保存の表示。`write(value)` が resolve したときだけ「保存済み」にする。
 *   keyguard(map, options)            ショートカットのキー。フォーカス中の要素の標準のキー操作を奪わない。
 *   viewstate(root, options)          読み込み中・失敗・空・中身の出し分け。失敗の出し先は、常に DOM にある。
 */
(function (root) {
  "use strict";

  /** 要素を作る。文字は `textContent` で入れる(HTML として解釈しない)。 */
  function make(tag, className, text) {
    var e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  /** 文字列、または引数を取って文字列を返す関数のどちらでも受ける。 */
  function label(l, arg) {
    return typeof l === "function" ? l(arg) : l;
  }

  function merge(defaults, given) {
    var out = {};
    var k;
    for (k in defaults) if (Object.prototype.hasOwnProperty.call(defaults, k)) out[k] = defaults[k];
    if (given) for (k in given) if (Object.prototype.hasOwnProperty.call(given, k)) out[k] = given[k];
    return out;
  }

  // ── saver: 保存の表示 ────────────────────────────────────────────────────────

  /**
   * 保存の状態を、`status`(`.status`)に出す。
   *  - `write(value)` は、書き込みの成功を確かめてから resolve する Promise(または値)を返す。reject するか、
   *    同期に throw するか、`false` を返したら、失敗として扱う。
   *  - 保存中 → 保存済み。失敗は、その項目の横に、再保存の操作(`.btn-link`)と一緒に出す。
   *  - 保存を続けて呼んだとき、古い保存の結果は、表示を書き換えない(最後の保存の結果だけが出る)。
   * options: labels({saving, saved, failed}。文字列、または関数)、retryLabel、retry(既存の再保存のボタン)。
   * 戻り値: { run(value) → Promise<boolean>, state() → "saving"|"saved"|"failed"|null, retry }
   */
  function saver(status, write, options) {
    if (!status || status.nodeType !== 1) throw new TypeError("saver: status は要素");
    if (typeof write !== "function") throw new TypeError("saver: write は関数");
    var o = options || {};
    var labels = merge(
      { saving: "保存中…", saved: "保存済み", failed: "保存できませんでした" },
      o.labels
    );
    var retry = o.retry || null;
    if (!retry) {
      retry = make("button", "btn-link", o.retryLabel || "もう一度保存");
      retry.type = "button";
      retry.hidden = true;
      status.insertAdjacentElement("afterend", retry);
    }
    if (!status.hasAttribute("role")) status.setAttribute("role", "status");

    var seq = 0;
    var last = { value: undefined };
    var current = null;

    function show(state, text) {
      current = state;
      status.setAttribute("data-state", state);
      status.textContent = text;
      retry.hidden = state !== "failed";
    }

    function run(value) {
      var mine = ++seq;
      last = { value: value };
      show("saving", label(labels.saving));
      var done;
      try {
        done = Promise.resolve(write(value));
      } catch (err) {
        done = Promise.reject(err);
      }
      return done
        .then(function (result) {
          if (result === false) throw new Error("write returned false");
          if (mine === seq) {
            show("saved", label(labels.saved, new Date()));
            status.dispatchEvent(new CustomEvent("saved", { bubbles: true, detail: { value: value } }));
          }
          return true;
        })
        .catch(function (err) {
          if (mine === seq) {
            show("failed", label(labels.failed, err));
            status.dispatchEvent(
              new CustomEvent("save-failed", { bubbles: true, detail: { error: err, value: value } })
            );
          }
          return false;
        });
    }

    retry.addEventListener("click", function () {
      run(last.value);
    });
    return {
      run: run,
      state: function () {
        return current;
      },
      retry: retry,
    };
  }

  // ── keyguard: ショートカットのキー ───────────────────────────────────────────

  var ARROWS_ETC = ["Home", "End", "PageUp", "PageDown"];

  /**
   * `target` が、キー `key` を、標準の操作として使うか(使うなら、ショートカットが奪ってはいけない)。
   *  - 文字を入れる欄(`textarea`・`select`・文字入力の `input`・`contenteditable`)は、`Escape` 以外のすべてのキー。
   *  - ボタン・`summary`・ボタンの `role`: Enter と Space。リンク: Enter。
   *  - ラジオ・タブなどの `role`、`input[type=radio]`・`range`: 矢印・Home・End など。
   */
  function stealsNative(target, key) {
    if (!target || target.nodeType !== 1) return false;
    var tag = target.tagName;
    var isArrow = key.indexOf("Arrow") === 0 || ARROWS_ETC.indexOf(key) >= 0;
    if (target.isContentEditable) return true;
    if (tag === "TEXTAREA" || tag === "SELECT") return key !== "Escape";
    if (tag === "INPUT") {
      var t = (target.type || "text").toLowerCase();
      if (t === "checkbox") return key === " ";
      if (t === "radio") return key === " " || isArrow;
      if (t === "button" || t === "submit" || t === "reset" || t === "image") {
        return key === "Enter" || key === " ";
      }
      if (t === "range") return isArrow;
      return key !== "Escape";
    }
    if (tag === "BUTTON" || tag === "SUMMARY") return key === "Enter" || key === " ";
    if (tag === "A" && target.hasAttribute("href")) return key === "Enter";
    var role = target.getAttribute("role");
    if (role === "button") return key === "Enter" || key === " ";
    if (role === "link") return key === "Enter";
    if (
      role === "radio" || role === "tab" || role === "menuitem" || role === "option" ||
      role === "slider" || role === "listbox" || role === "tablist" || role === "radiogroup"
    ) {
      return isArrow || key === " " || key === "Enter";
    }
    return false;
  }

  /**
   * ショートカットのキーを登録する。`map` は、`KeyboardEvent.key`(Space は `" "`)→ ハンドラ。
   *  - フォーカス中の要素が、そのキーを標準の操作に使うなら、ハンドラを呼ばない(ボタンの Enter・Space を奪わない)。
   *  - Ctrl・Meta・Alt つきのキーと、変換中(IME)のキーは、ハンドラを呼ばない(`options.modifiers` が真なら、修飾キーつきも呼ぶ)。
   *  - ハンドラが `false` を返さなければ、`preventDefault()` する(Space のスクロールなど)。
   * options: scope(既定は document)、modifiers。
   * 戻り値: { off() }(登録を外す)
   */
  function keyguard(map, options) {
    var o = options || {};
    var scope = o.scope || document;
    function onKey(e) {
      if (e.defaultPrevented || e.isComposing || typeof e.key !== "string") return;
      if ((e.ctrlKey || e.metaKey || e.altKey) && !o.modifiers) return;
      var handler = Object.prototype.hasOwnProperty.call(map, e.key) ? map[e.key] : null;
      if (typeof handler !== "function") return;
      if (stealsNative(e.target, e.key)) return;
      if (handler(e) !== false) e.preventDefault();
    }
    scope.addEventListener("keydown", onKey);
    return {
      off: function () {
        scope.removeEventListener("keydown", onKey);
      },
    };
  }

  // ── viewstate: 読み込み中・失敗・空・中身 ─────────────────────────────────────

  /** 空とみなす値(null・undefined・長さ 0 の配列)。 */
  function isEmptyData(data) {
    return data === null || data === undefined || (Array.isArray(data) && data.length === 0);
  }

  /**
   * `root` の中に、読み込み中・失敗・空・中身の 4 つの出し分けを作る。失敗の出し先(`role="alert"`)は、最初から DOM にある
   * (初回の読み込みの失敗で、出し先が無く、「読み込み中…」のまま止まる型を作らない)。
   * options: labels({loading, failed, empty, retry})、isEmpty(data → boolean)
   * 戻り値: { loading(), ready(render), empty(text), failed(err, onRetry), run(fetcher, render), content, error }
   *   run(fetcher, render): `fetcher()` の結果を、空なら空の表示、そうでなければ `render(data, content)` で中身に描く。
   *   失敗は、再読み込みの操作つきで出す。続けて呼んだとき、古い結果は、表示を書き換えない。
   */
  function viewstate(rootEl, options) {
    if (!rootEl || rootEl.nodeType !== 1) throw new TypeError("viewstate: root は要素");
    var o = options || {};
    var labels = merge(
      {
        loading: "読み込み中…",
        failed: "読み込めませんでした。通信を確かめて、もう一度開いてください。",
        empty: "まだありません",
        retry: "もう一度読み込む",
      },
      o.labels
    );
    var isEmpty = typeof o.isEmpty === "function" ? o.isEmpty : isEmptyData;

    var loadingEl = make("p", "status", labels.loading);
    loadingEl.setAttribute("role", "status");
    loadingEl.setAttribute("data-state", "saving");
    var errorEl = make("p", "alert alert-ng");
    errorEl.setAttribute("role", "alert");
    var retryEl = make("button", "btn-link", labels.retry);
    retryEl.type = "button";
    var emptyEl = make("p", "empty", labels.empty);
    var contentEl = make("div");
    var parts = [loadingEl, errorEl, retryEl, emptyEl, contentEl];
    var onRetry = null;
    var seq = 0;

    parts.forEach(function (p) {
      p.hidden = true;
      rootEl.appendChild(p);
    });

    function only(visible) {
      parts.forEach(function (p) {
        p.hidden = visible.indexOf(p) < 0;
      });
      rootEl.setAttribute("aria-busy", visible.indexOf(loadingEl) >= 0 ? "true" : "false");
    }

    function loading() {
      only([loadingEl]);
    }
    function ready(render) {
      while (contentEl.firstChild) contentEl.removeChild(contentEl.firstChild);
      if (typeof render === "function") render(contentEl);
      only([contentEl]);
    }
    function empty(text) {
      if (text !== undefined) emptyEl.textContent = text;
      only([emptyEl]);
    }
    function failed(err, retryFn) {
      errorEl.textContent = label(labels.failed, err);
      onRetry = typeof retryFn === "function" ? retryFn : null;
      only(onRetry ? [errorEl, retryEl] : [errorEl]);
      rootEl.dispatchEvent(new CustomEvent("view-failed", { bubbles: true, detail: { error: err } }));
    }

    retryEl.addEventListener("click", function () {
      if (onRetry) onRetry();
    });

    function run(fetcher, render) {
      var mine = ++seq;
      loading();
      var got;
      try {
        got = Promise.resolve(fetcher());
      } catch (err) {
        got = Promise.reject(err);
      }
      return got.then(
        function (data) {
          if (mine !== seq) return true;
          if (isEmpty(data)) empty();
          else
            ready(function (c) {
              if (typeof render === "function") render(data, c);
            });
          return true;
        },
        function (err) {
          if (mine === seq) {
            failed(err, function () {
              run(fetcher, render);
            });
          }
          return false;
        }
      );
    }

    return {
      loading: loading,
      ready: ready,
      empty: empty,
      failed: failed,
      run: run,
      content: contentEl,
      error: errorEl,
    };
  }

  root.pageBehavior = { saver: saver, keyguard: keyguard, viewstate: viewstate };
})(typeof window !== "undefined" ? window : globalThis);
