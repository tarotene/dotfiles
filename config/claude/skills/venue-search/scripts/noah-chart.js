// noah-chart.js — ピアノスタジオノア(grandpiano.jp)のスタジオ予約検索
// (https://www.grandpiano.jp/noahweb/webs/chart/)の DOM から週間空き状況を
// 読み取るための browser_evaluate 用スニペット集(実測 2026-09-28)。
//
// このページは JS で描画され、選択状態は URL に反映されない(ブックマーク
// 不可・共有不可)。そのため「決定的に URL を組む」ことができず、毎回
// Playwright で操作し直す必要がある — venue-urls.sh がこのサービスの
// url を固定のトップページだけに留めている理由(SKILL.md「3.1」参照)。
//
// 使い方: SKILL.md の手順に従い、page.evaluate に以下の関数本体を渡す
// (browser_evaluate ツールの `function` 引数にそのままコピーする)。

// --- 現在表示中の週のヘッダ情報を返す ---
// 戻り値: { month: "2026.11", days: ["2月","3火",...,"8日"] }
function noahWeekHeader() {
  const month = Array.from(document.querySelectorAll('*'))
    .filter((e) => e.children.length === 0 && /^\d{4}\.\d{2}$/.test(e.textContent.trim()))
    .map((e) => e.textContent.trim());
  const days = Array.from(document.querySelectorAll('.cal_status_week h3')).map((h) =>
    h.textContent.replace(/\s+/g, '')
  );
  return { month, days };
}

// --- 指定した曜日ラベル(例: "7土")の時間帯別空き状況を返す ---
// 戻り値: [{t: "9:30", avail: true}, ...] — avail=false は "×"(空き無し)。
// t は各行の時刻ラベル(表示は30分オフセットだが、1時間単位の枠に対応する
// — 例えば "9:30" の行は 9:00〜10:00 の枠を指す。実測で確認済み)。
// timeFilter を渡すと該当ラベルだけに絞る(省略時は全24行)。
function noahDaySlots(dayLabel, timeFilter) {
  const rowLabels = Array.from(document.querySelectorAll('ul.cal_time_list li')).map((li) =>
    li.textContent.trim()
  );
  const dayDivs = Array.from(document.querySelectorAll('.cal_status_week'));
  const target = dayDivs.find(
    (div) => div.querySelector('h3').textContent.replace(/\s+/g, '') === dayLabel
  );
  if (!target) return null;
  const lis = Array.from(target.querySelectorAll('ul > li'));
  const all = lis.map((li, i) => ({ t: rowLabels[i], avail: li.textContent.trim() !== '×' }));
  return timeFilter ? all.filter((x) => timeFilter.includes(x.t)) : all;
}

// --- 「翌週」「前週」ボタンのセレクタ(browser_click の target にそのまま渡せる) ---
// disabled になったら(class に "_disabled" が付く)それ以上その方向へは
// 進めない — 実測 2026-09-28、今日から約13週先(3か月強)が上限だった。
// bash `date -d "+13 weeks"` 相当の範囲だけが表示可能、と見積もっておく。
const NOAH_NEXT_WEEK_SELECTOR = '.ctrl_btn._next';
const NOAH_PREV_WEEK_SELECTOR = '.ctrl_btn._prev';

// --- 現在選択中のスタジオの料金表示を返す(例: ["￥1,870～", "￥-"]) ---
function noahSelectedPrice() {
  return Array.from(document.querySelectorAll('*'))
    .filter((e) => e.children.length === 0 && /^￥/.test(e.textContent.trim()))
    .map((e) => e.textContent.trim());
}
