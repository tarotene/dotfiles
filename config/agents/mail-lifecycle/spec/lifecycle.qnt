// 未裁定のメール項目の寿命の仕様(Quint)。
//
// 読み方は docs/agents/mail-lifecycle.md の「仕様と検査」の節にある。
// このファイルは論理だけを持つ。次の 2 つは検査のたびに生成する(コミットしない):
//   - ./vocab.qnt  vocabulary.json から jq で生成する。遷移の可否の正本は語彙 JSON。
//   - ./cfg.qnt    scenarios.json の 1 件から生成する。規則の有効・無効と環境の絞り込み。
//
// 項目(key)は、スレッドと、それを開いたメールの受信(receipt)の組である。キーは 0..3 で、
// キー k のスレッドは thr(k)、同じスレッドの中の受信の順はキーの順(古い方が小さい)。
// 項目の状態は保存しない。vault のノートの行(追記のみの出来事の列)を、先頭から畳み込んで導く。
//
// 参加者:
//   エージェント(agent)  メールを読む。開く出来事(read/draft/ask)と apply/move を書く。
//                          新しい受信がスレッドの未裁定の項目の後に来たら、先にその項目の move を書き、
//                          そのあとで新しい項目を開く。draft は行の記録と Gmail の下書きの作成の 2 手。
//   本人(person)         decide/pass を書く。答えないこともある(公平性の仮定を置かない)。
//                          Gmail の下書きを消す・送る。ノートの行を 1 本だけ消す(未裁定の行、終端の行)。
//   掃引(sweep)          remind/stale だけを書く。規則が許す前には書かない。
// 機体は 2 つ(TWO_MACHINES)。それぞれ vault のノートの行を自分の写し(views)に持ち、
// 相手の機体が書いた行は LiveSync の遅延(inbox)を経て、書かれた順に届く。
// 機体ごとに、終端の出来事を書いたあとで自分の台帳(控え、led)に項目を足す。足す前に落ちると
// 控えが欠ける(余計な控えはできない)。
//
// 畳み込みの規則(語彙 JSON の表が正本。applyLine):
//   - 出来事は行の順に当てる。語彙の from に今の状態が無い出来事は、状態を変えず食い違い(disc)に数える。
//     終端の状態からの遷移は語彙に無いので、終端は吸収する(ABSORB を外すと、終端のあとの出来事も当たる)。
//   - 開く出来事だけで from が ["none"] の出来事(read/draft/ask)が、開いた項目に重ねて来たら、
//     何もせず無視する(再読の冪等。DUP_IDEMPOTENT を外すと、開いた直後の状態に戻す)。
//   - 項目を新しく開くとき、同じスレッドの古い項目が未裁定なら、暗黙に superseded にする(行は書かれない)。
//     新しい項目が既に未裁定なら、いま開いた古い項目の方を superseded にする(2 機体の行の統合)。
//     エージェントが先に move を書いていれば、この暗黙の規則は働かない。働いたとき、開いた行の
//     bl(書き手が、古い未裁定の項目を見ていなかった)が偽なら、規律の破れとして数える(badImp)。
//
// 時間: 日は 0..CAP を回る。CAP を超えて進むときは、すべての日付を 1 つ戻して今日を CAP に保つ
// (下限は 0 で止める。閾値は CAP 以下なので、古さの判定は変わらない)。こうして日が無限に進む
// 公平な実行を、有限の状態で見る。
//
// 検査しない性質(仕様の限界):
//   - 終端の行の削除の検知(s7)は、台帳を持つ機体が、自分の写しで削除されたときだけ。
//     台帳を持たない機体(相手の機体)で消されたとき、その機体は検知しない。LiveSync が削除を
//     相手に伝える挙動は、仕様に入れていない(削除は消した機体の写しだけに効く)。
//   - 2 つの機体で行が届く順は異なりうる。畳み込みは行の順に依るので、機体の間で最終の状態が
//     食い違うことがある(収束は性質に入れていない)。同じ受信に対する開く出来事は、機体によらず
//     受信の内容から一意に決まる(rc.kind)と仮定している。
//   - 実時間の閾値(7 日・7 日・3 日)は、仕様では小さい値(scenarios.json の REMIND_AFTER など)に置き換える。
module lifecycle {
  import vocab.* from "./vocab"
  import cfg.* from "./cfg"

  // ---------------------------------------------------------------- 型
  // k: 項目、ev: 出来事、d: 書いた日、due: 期日(-1 = 無し、開く行だけ)、
  // bl: 開く行の書き手が、古い未裁定の項目を見ていなかった(ゴースト)、
  // chk: stale の行の書き手が、その時点で「最後の非 remind のあとに、今日より前の日の remind がある」を満たしていた(ゴースト)
  type Line = { k: int, ev: str, d: int, due: int, bl: bool, chk: bool }
  type Rc = { arr: bool, kind: str, due: int }
  type Ag = { pk: int, mode: str }
  // st: 状態、terms: 終端へ入った回数、disc: 食い違いの数、L: 最後の非 remind の日、
  // hasR/R: 最後の非 remind のあとの remind とその日、due: 期日
  type Acc = { st: str, terms: int, disc: int, L: int, hasR: bool, R: int, due: int }
  type Fo = { a: int -> Acc, badImp: bool }
  type Sys = {
    views: int -> List[Line],     // 機体ごとの、ノートの行の写し
    fo: int -> Fo,                // 機体ごとの、写しの畳み込みの結果(行を足すたびに 1 行ぶん進める。cacheOk が foldView と一致を確かめる)
    inbox: int -> List[Line],     // LiveSync で、まだ届いていない行
    rc: int -> Rc,                // 受信(受信箱に来たメール)
    gm: int -> str,               // Gmail の下書き: none | exists | sent | deleted
    ag: int -> Ag,                // エージェントの、2 手の途中(pk = -1 は空き)
    led: int -> Set[int],         // 機体ごとの台帳(控え)
    todo: int -> Set[int],        // 終端の行を書いたが、台帳にまだ足していない項目
    wrote: int -> Set[int],       // ゴースト: 台帳の手順を最後まで終えた項目(台帳の規則に依らない)
    today: int,
    crashes: int,
    delU: int,
    delT: int,
    ops: int,                     // 本人が書いた decide/pass の数
    fired: Set[Tr],               // ゴースト: 直前の手が書いた出来事(語彙との照合用)
    s1bad: bool,                  // ゴースト: 終端が吸収されなかった手があった
    s5bad: bool,                  // ゴースト: 重ねて届いた開く出来事が状態を変えた
    badRem: bool,                 // ゴースト: 掃引が、終端の行を消された項目に remind/stale を書いた
    tried: Set[int],              // ゴースト: エージェントが下書きの項目を始めた
    rec: Set[int],                // ゴースト: 行が一度でも書かれた項目
  }

  var s: Sys

  // ---------------------------------------------------------------- 定数
  pure val KEYS = Set(0, 1, 2, 3)
  pure val NKEYS = N_KEYS
  pure val MACH2 = Set(0, 1)
  pure val AKEYS = KEYS.filter(k => k < NKEYS)
  pure val MACH = if (TWO_MACHINES) Set(0, 1) else Set(0)
  pure val KINDS = Set("read", "draft", "ask").filter(x =>
    (x == "read" and ALLOW_READ) or (x == "draft" and ALLOW_DRAFT) or (x == "ask" and ALLOW_ASK))
  pure val DELTAS = if (WITH_DUE) Set(-1, 1, 2) else Set(-1)

  pure def thr(k: int): int = if (k >= 2) 1 else 0
  pure def prevK(k: int): int = if (k > 0) k - 1 else 0

  pure val noLines: List[Line] = []
  pure val noRc: Rc = { arr: false, kind: "", due: -1 }
  pure val idleAg: Ag = { pk: -1, mode: "" }
  pure val accInit: Acc = { st: INITIAL, terms: 0, disc: 0, L: 0, hasR: false, R: 0, due: -1 }
  pure val foInit: Fo = { a: KEYS.mapBy(k => accInit), badImp: false }

  // ---------------------------------------------------------------- 畳み込み(語彙が正本)
  pure def isTerm(st: str): bool = TERMINAL.contains(st)
  pure def isUnres(st: str): bool = UNRESOLVED.contains(st)

  // 語彙で、状態 st から出来事 ev で行き着く状態。無ければ ""
  pure def target(ev: str, st: str): str = {
    val ts = TRANSITIONS_LIST.select(t => t.ev == ev and t.from == st)
    if (ts.length() > 0) ts.head().to else ""
  }
  // ABSORB を外したときの、from を見ない行き先(状態を変える出来事の to。変えない出来事は今のまま)
  pure def anyTarget(ev: str, st: str): str = {
    val ts = TRANSITIONS_LIST.select(t => t.ev == ev and t.to != t.from)
    if (ts.length() > 0) ts.head().to else st
  }

  pure def applyLine(fo: Fo, l: Line): Fo = {
    val x = fo.a.get(l.k)
    val opening = OPENING.contains(l.ev)
    val seenKey = x.st != INITIAL
    val tg = target(l.ev, x.st)
    val ignoredDup = opening and seenKey and DUP_IDEMPOTENT
    val reopen = opening and seenKey and not(DUP_IDEMPOTENT)
    val forced = not(ABSORB) and tg == "" and isTerm(x.st) and not(opening)
    val applied = not(ignoredDup) and (tg != "" or reopen or forced)
    val st2 = if (reopen) target(l.ev, INITIAL) else if (forced) anyTarget(l.ev, x.st) else tg
    val isRem = l.ev == "remind"
    val x2: Acc =
      if (ignoredDup) x
      else if (applied) {
        ...x,
        st: st2,
        terms: x.terms + (if (isTerm(st2) and st2 != x.st) 1 else 0),
        L: if (isRem) x.L else l.d,
        hasR: isRem,
        R: if (isRem) l.d else x.R,
        due: if (seenKey) x.due else l.due,
      }
      else { ...x, disc: x.disc + 1 }
    // 新しく開いた項目と、同じスレッドの項目との暗黙の supersede。
    // 同じスレッドは 2 つのキー(偶数が古く、奇数が新しい)だけなので、相手は 1 つに決まる。
    // mapBy を畳み込みの中で重ねると、TLC の ENABLED の評価で遅延評価が入れ子になって爆発するので、
    // set で 1 つずつ書き換える。
    val sib = if (l.k % 2 == 0) l.k + 1 else l.k - 1
    val fresh = opening and applied and not(seenKey)
    val sibSt = fo.a.get(sib).st
    val impOld = fresh and l.k % 2 == 1 and isUnres(sibSt)
    val impNew = fresh and isUnres(st2) and l.k % 2 == 0 and isUnres(sibSt)
    val self: Acc = if (impNew) { ...x2, st: "superseded", terms: x2.terms + 1, hasR: false } else x2
    val a1 = fo.a.set(l.k, self)
    val a2 = if (impOld) a1.set(sib, { ...fo.a.get(sib), st: "superseded", terms: fo.a.get(sib).terms + 1, hasR: false }) else a1
    { a: a2, badImp: fo.badImp or (impOld and not(l.bl)) }
  }

  pure def foldView(ls: List[Line]): Fo = ls.foldl(foInit, (f, l) => applyLine(f, l))
  pure def hasLine(ls: List[Line], k: int): bool = ls.select(l => l.k == k).length() > 0
  pure def hasTerm(ls: List[Line], k: int): bool =
    ls.select(l => l.k == k and TERM_EVENTS.contains(l.ev)).length() > 0

  // 終端の行を、書いたのとは別に消された項目を見つける(台帳の控えと、写しの行の突き合わせ。純粋な関数)
  pure def check(ls: List[Line], led: Set[int]): Set[int] = led.filter(k => not(hasTerm(ls, k)))

  // ---------------------------------------------------------------- 読み取り
  // 畳み込みの結果は状態に持つ。TLC の ENABLED の評価(公平性)で foldl を毎回たどると、
  // 行の数に対して指数的に遅くなる(TLC 2.19 で実測)。
  def fm(m: int): Fo = s.fo.get(m)
  def stOf(m: int, k: int): str = fm(m).a.get(k).st
  def flagged(m: int): Set[int] = check(s.views.get(m), s.led.get(m))
  def handled(m: int, k: int): bool =
    hasLine(s.views.get(m), k) or s.gm.get(k) != "none" or s.led.get(m).contains(k)
  def olderHandled(m: int, k: int): bool = k % 2 == 0 or handled(m, prevK(k))
  def idle(m: int): bool = s.ag.get(m).pk < 0

  // 機体 m の写しで、k より古い、同じスレッドの未裁定の項目
  def olderUnres(m: int, k: int): Set[int] =
    KEYS.filter(j => thr(j) == thr(k) and j < k and isUnres(stOf(m, j)) and not(flagged(m).contains(j)))

  pure def mkLine(k: int, ev: str, d: int, due: int, bl: bool, chk: bool): Line =
    { k: k, ev: ev, d: d, due: due, bl: bl, chk: chk }

  // ---------------------------------------------------------------- 書き込みの部品
  pure def addLine(x: Sys, m: int, l: Line): Sys = {
    ...x,
    views: x.views.set(m, x.views.get(m).append(l)),
    fo: x.fo.set(m, applyLine(x.fo.get(m), l)),
    inbox: if (TWO_MACHINES) x.inbox.set(1 - m, x.inbox.get(1 - m).append(l)) else x.inbox,
    todo: if (WITH_LEDGER and TERM_EVENTS.contains(l.ev)) x.todo.set(m, x.todo.get(m).union(Set(l.k))) else x.todo,
    rec: x.rec.union(Set(l.k)),
  }
  pure def addLines(x: Sys, m: int, ls: List[Line]): Sys = ls.foldl(x, (a, l) => addLine(a, m, l))

  // 古い項目の move を先に書く(MOVE_ON_NEW)
  def moveLines(m: int, k: int): List[Line] =
    if (MOVE_ON_NEW)
      olderUnres(m, k).fold(noLines, (acc, j) => acc.append(mkLine(j, "move", s.today, -1, true, true)))
    else noLines
  def moveTrs(m: int, k: int): Set[Tr] =
    if (MOVE_ON_NEW)
      olderUnres(m, k).map(j => { ev: "move", from: stOf(m, j), to: "superseded", by: "agent" })
    else Set()

  pure def slideLine(l: Line): Line = {
    ...l,
    d: if (l.d > 0) l.d - 1 else 0,
    due: if (l.due > 0) l.due - 1 else l.due,
  }
  pure def slideAll(ls: List[Line]): List[Line] = ls.foldl(noLines, (acc, l) => acc.append(slideLine(l)))
  pure def slideAcc(x: Acc): Acc = {
    ...x,
    L: if (x.L > 0) x.L - 1 else 0,
    R: if (x.R > 0) x.R - 1 else 0,
    due: if (x.due > 0) x.due - 1 else x.due,
  }
  pure def slideFo(f: Fo): Fo = { ...f, a: KEYS.mapBy(k => slideAcc(f.a.get(k))) }

  pure def s1ViolM(b: List[Line], a: List[Line]): bool = {
    val fb = foldView(b)
    val fa = foldView(a)
    KEYS.exists(k => isTerm(fb.a.get(k).st) and fa.a.get(k).st != fb.a.get(k).st)
  }
  pure def s1Viol(before: int -> List[Line], after: int -> List[Line]): bool =
    MACH.exists(m => before.get(m) != after.get(m) and s1ViolM(before.get(m), after.get(m)))

  // 重ねて届いた開く出来事が、状態を変える
  pure def dupChanges(ls: List[Line], l: Line): bool = {
    val before = foldView(ls).a.get(l.k).st
    OPENING.contains(l.ev) and before != INITIAL and foldView(ls.append(l)).a.get(l.k).st != before
  }

  // 手の確定。nx は s から作った次の状態、trs はその手が書いた出来事、del は行の削除(追記のみの前提の外)
  action commit(nx: Sys, trs: Set[Tr], del: bool): bool = s' = {
    ...nx,
    fired: if (TRACK_FIRED) trs else Set(),
    s1bad: nx.s1bad or (TRACK_S1 and not(del) and s1Viol(s.views, nx.views)),
  }

  // ---------------------------------------------------------------- 初期状態
  action init = s' = {
    views: MACH2.mapBy(m => noLines),
    fo: MACH2.mapBy(m => foInit),
    inbox: MACH2.mapBy(m => noLines),
    rc: KEYS.mapBy(k => noRc),
    gm: KEYS.mapBy(k => "none"),
    ag: MACH2.mapBy(m => idleAg),
    led: MACH2.mapBy(m => Set()),
    todo: MACH2.mapBy(m => Set()),
    wrote: MACH2.mapBy(m => Set()),
    today: 0,
    crashes: 0,
    delU: 0,
    delT: 0,
    ops: 0,
    fired: Set(),
    s1bad: false,
    s5bad: false,
    badRem: false,
    tried: Set(),
    rec: Set(),
  }

  // ---------------------------------------------------------------- 環境
  // 受信が来る。同じスレッドの中では順に来る。開く出来事の種類と期日は、受信の内容で決まる
  action arrive(k: int): bool = {
    nondet kd = KINDS.oneOf()
    nondet dl = DELTAS.oneOf()
    all {
      k < NKEYS, not(s.rc.get(k).arr),
      k % 2 == 0 or s.rc.get(prevK(k)).arr,
      commit({ ...s, rc: s.rc.set(k, { arr: true, kind: kd, due: if (dl < 0) -1 else s.today + dl }) },
             Set(), false),
    }
  }

  // 日が進む。CAP に達していれば、日付を 1 つ戻して今日を保つ
  action tick: bool = {
    val sl = s.today >= CAP
    all { WITH_TIME, commit({
      ...s,
      today: if (sl) CAP else s.today + 1,
      views: if (sl) MACH2.mapBy(m => slideAll(s.views.get(m))) else s.views,
      inbox: if (sl) MACH2.mapBy(m => slideAll(s.inbox.get(m))) else s.inbox,
      fo: if (sl) MACH2.mapBy(m => slideFo(s.fo.get(m))) else s.fo,
      rc: if (sl) KEYS.mapBy(k => { ...s.rc.get(k), due: if (s.rc.get(k).due > 0) s.rc.get(k).due - 1 else s.rc.get(k).due })
          else s.rc,
    }, Set(), false) }
  }

  // LiveSync: 相手の機体が書いた行が、遅れて届く
  action deliver(m: int): bool = {
    val ib = s.inbox.get(m)
    all {
      MACH.contains(m), ib.length() > 0,
      commit({
        ...s,
        views: s.views.set(m, s.views.get(m).append(ib.head())),
        fo: s.fo.set(m, applyLine(s.fo.get(m), ib.head())),
        inbox: s.inbox.set(m, ib.tail()),
        s5bad: s.s5bad or (TRACK_S5 and dupChanges(s.views.get(m), ib.head())),
      }, Set(), false),
    }
  }

  // エージェントが、下書きの 2 手の途中で落ちる(続きを失う)
  action agentCrash(m: int): bool = all {
    CAN_CRASH, MACH.contains(m), s.crashes < MAX_CRASHES, s.ag.get(m).pk >= 0,
    commit({ ...s, ag: s.ag.set(m, idleAg), crashes: s.crashes + 1 }, Set(), false),
  }
  // 終端の行を書いたあと、台帳に足す前に落ちる(控えが欠ける)
  action ledgerCrash(m: int, k: int): bool = all {
    CAN_CRASH, MACH.contains(m), s.crashes < MAX_CRASHES, s.todo.get(m).contains(k),
    commit({ ...s, todo: s.todo.set(m, s.todo.get(m).exclude(Set(k))), crashes: s.crashes + 1 }, Set(), false),
  }

  // ---------------------------------------------------------------- 機体(台帳)
  action ledgerAppend(m: int, k: int): bool = all {
    MACH.contains(m), s.todo.get(m).contains(k),
    commit({
      ...s,
      todo: s.todo.set(m, s.todo.get(m).exclude(Set(k))),
      led: if (LEDGER) s.led.set(m, s.led.get(m).union(Set(k))) else s.led,
      wrote: s.wrote.set(m, s.wrote.get(m).union(Set(k))),
    }, Set(), false),
  }

  // ---------------------------------------------------------------- エージェント
  // 受信を読む。記録した行が無く、Gmail にも下書きが無い受信が対象
  action agentOpen(m: int, k: int): bool = {
    val kd = s.rc.get(k).kind
    val blind = olderUnres(m, k).size() == 0
    val lines = moveLines(m, k).append(mkLine(k, kd, s.today, s.rc.get(k).due, blind, true))
    val trs = moveTrs(m, k).union(Set({ ev: kd, from: INITIAL, to: if (kd == "read") "closed" else "awaiting", by: "agent" }))
    all {
      MACH.contains(m), s.rc.get(k).arr, idle(m), not(handled(m, k)), olderHandled(m, k),
      any {
        all { kd != "draft", commit(addLines(s, m, lines), trs, false) },
        // 行を先に記録し、あとで Gmail の下書きを作る
        all {
          kd == "draft", RECORD_FIRST,
          commit({ ...addLines(s, m, lines), ag: s.ag.set(m, { pk: k, mode: "drafting" }),
                   tried: s.tried.union(Set(k)) }, trs, false),
        },
        // Gmail の下書きを先に作り、あとで行を記録する
        all {
          kd == "draft", not(RECORD_FIRST),
          commit({ ...s, gm: s.gm.set(k, "exists"), ag: s.ag.set(m, { pk: k, mode: "recording" }),
                   tried: s.tried.union(Set(k)) }, Set(), false),
        },
      }
    }
  }
  action createDraft(m: int): bool = {
    val a = s.ag.get(m)
    val k = if (a.pk < 0) 0 else a.pk
    all {
      MACH.contains(m), a.pk >= 0, a.mode == "drafting",
      commit({
        ...s,
        gm: if (s.gm.get(k) == "none" and isUnres(stOf(m, k))) s.gm.set(k, "exists") else s.gm,
        ag: s.ag.set(m, idleAg),
      }, Set(), false),
    }
  }
  action recordLine(m: int): bool = {
    val a = s.ag.get(m)
    val k = if (a.pk < 0) 0 else a.pk
    val blind = olderUnres(m, k).size() == 0
    val lines = moveLines(m, k).append(mkLine(k, "draft", s.today, s.rc.get(k).due, blind, true))
    val trs = moveTrs(m, k).union(Set({ ev: "draft", from: INITIAL, to: "awaiting", by: "agent" }))
    all {
      MACH.contains(m), a.pk >= 0, a.mode == "recording",
      commit({ ...addLines(s, m, lines), ag: s.ag.set(m, idleAg) }, trs, false),
    }
  }
  // 行はあるのに Gmail に下書きが無い下書きの項目を、走査で作り直す(落ちた後の回復)
  action redraft(m: int, k: int): bool = all {
    MACH.contains(m), idle(m), s.gm.get(k) == "none",
    s.views.get(m).select(l => l.k == k and l.ev == "draft").length() > 0,
    isUnres(stOf(m, k)), not(flagged(m).contains(k)),
    commit({ ...s, gm: s.gm.set(k, "exists") }, Set(), false),
  }
  // 決定を実行する、または本人が下書きを送ったのを見て閉じる
  action agentApply(m: int, k: int): bool = all {
    MACH.contains(m), idle(m), isUnres(stOf(m, k)), not(flagged(m).contains(k)),
    stOf(m, k) == "decided" or s.gm.get(k) == "sent",
    commit(addLine(s, m, mkLine(k, "apply", s.today, -1, true, true)),
           Set({ ev: "apply", from: stOf(m, k), to: "closed", by: "agent" }), false),
  }

  // ---------------------------------------------------------------- 本人
  action personDecide(m: int, k: int): bool = all {
    CAN_DECIDE, MACH.contains(m), s.ops < MAX_PERSON,
    stOf(m, k) == "awaiting", not(flagged(m).contains(k)),
    commit({ ...addLine(s, m, mkLine(k, "decide", s.today, -1, true, true)), ops: s.ops + 1 },
           Set({ ev: "decide", from: "awaiting", to: "decided", by: "person" }), false),
  }
  action personPass(m: int, k: int): bool = all {
    CAN_PASS, MACH.contains(m), s.ops < MAX_PERSON, s.rc.get(k).arr,
    stOf(m, k) == INITIAL or isUnres(stOf(m, k)), not(flagged(m).contains(k)),
    commit({ ...addLine(s, m, mkLine(k, "pass", s.today, -1, true, true)), ops: s.ops + 1 },
           Set({ ev: "pass", from: stOf(m, k), to: "dismissed", by: "person" }), false),
  }
  action gmailDelete(k: int): bool = all {
    CAN_DEL_DRAFT, s.gm.get(k) == "exists",
    commit({ ...s, gm: s.gm.set(k, "deleted") }, Set(), false),
  }
  action gmailSend(k: int): bool = all {
    CAN_SEND, s.gm.get(k) == "exists",
    commit({ ...s, gm: s.gm.set(k, "sent") }, Set(), false),
  }
  // ノートの行を 1 本だけ消す。未裁定の行(終端の出来事でない行)
  action delUnres(m: int): bool = {
    val ls = s.views.get(m)
    nondet i = ls.indices().oneOf()
    all {
      CAN_DEL_U, MACH.contains(m), s.delU == 0, not(TERM_EVENTS.contains(ls.nth(i).ev)),
      commit({ ...s, views: s.views.set(m, ls.slice(0, i).concat(ls.slice(i + 1, ls.length()))),
               fo: s.fo.set(m, foldView(ls.slice(0, i).concat(ls.slice(i + 1, ls.length())))), delU: 1 },
             Set(), true),
    }
  }
  // 終端の行を 1 本だけ消す(検知されるべき場合)
  action delTerm(m: int): bool = {
    val ls = s.views.get(m)
    nondet i = ls.indices().oneOf()
    all {
      CAN_DEL_T, MACH.contains(m), s.delT == 0, TERM_EVENTS.contains(ls.nth(i).ev),
      commit({ ...s, views: s.views.set(m, ls.slice(0, i).concat(ls.slice(i + 1, ls.length()))),
               fo: s.fo.set(m, foldView(ls.slice(0, i).concat(ls.slice(i + 1, ls.length())))), delT: 1 },
             Set(), true),
    }
  }

  // ---------------------------------------------------------------- 掃引(remind / stale だけ)
  // R: 最後の非 remind のあとの remind の日。期日 D があれば、その 3 日前(BEFORE_DUE)から remind できる
  def remindOk(m: int, k: int): bool = {
    val x = fm(m).a.get(k)
    isUnres(x.st) and not(x.hasR) and
    (s.today - x.L >= REMIND_AFTER or (x.due >= 0 and s.today >= x.due - BEFORE_DUE))
  }
  // remind が既にあり、それが今日より前の日で、remind から STALE_AFTER 日経つか期日を過ぎている
  def staleOk(m: int, k: int): bool = {
    val x = fm(m).a.get(k)
    isUnres(x.st) and (
      (x.hasR and (not(STALE_NEXT_DAY) or x.R < s.today) and
       (s.today - x.R >= STALE_AFTER or (x.due >= 0 and s.today > x.due)))
      or (not(NEED_REMIND) and not(x.hasR) and s.today - x.L >= REMIND_AFTER))
  }
  def skipOk(m: int, k: int): bool = not(SKIP_FLAGGED) or not(flagged(m).contains(k))

  action sweepRemind(m: int, k: int): bool = all {
    MACH.contains(m), SWEEP_ALL or m == 0, remindOk(m, k), skipOk(m, k),
    commit({ ...addLine(s, m, mkLine(k, "remind", s.today, -1, true, true)),
             badRem: s.badRem or flagged(m).contains(k) },
           Set({ ev: "remind", from: stOf(m, k), to: stOf(m, k), by: "machine" }), false),
  }
  action sweepStale(m: int, k: int): bool = {
    val x = fm(m).a.get(k)
    all {
      MACH.contains(m), SWEEP_ALL or m == 0, staleOk(m, k), skipOk(m, k),
      commit({ ...addLine(s, m, mkLine(k, "stale", s.today, -1, true, x.hasR and x.R < s.today)),
               badRem: s.badRem or flagged(m).contains(k) },
             Set({ ev: "stale", from: x.st, to: "stale", by: "machine" }), false),
    }
  }

  // 何も起きない手(全員が止まった状態を行き止まりにしない。活性は公平性の仮定のもとで見る)
  action stutter = s' = s

  // 分岐ごとに必要な nondet だけを持たせる(使わない引数の直積を TLC に列挙させない)
  action stepArrive = { nondet k = KEYS.oneOf()
                        arrive(k) }
  action stepMk = {
    nondet m = MACH.oneOf()
    nondet k = KEYS.oneOf()
    any { agentOpen(m, k), redraft(m, k), agentApply(m, k), sweepRemind(m, k), sweepStale(m, k),
          ledgerAppend(m, k), ledgerCrash(m, k), personDecide(m, k), personPass(m, k) }
  }
  action stepMachine = {
    nondet m = MACH.oneOf()
    any { deliver(m), createDraft(m), recordLine(m), agentCrash(m), delUnres(m), delTerm(m) }
  }
  action stepGmail = {
    nondet k = KEYS.oneOf()
    any { gmailDelete(k), gmailSend(k) }
  }

  action step = any { stepArrive, stepMk, stepMachine, stepGmail, tick, stutter }

  // ================================================================ 性質
  // S1: 終端は吸収される。追記のみの行の列で、終端だった項目の状態が変わらない(行の削除の手を除く)
  val s1 = not(s.s1bad)
  // S2: 1 つの項目は、終端へ高々 1 回しか入らない
  val s2 = MACH.forall(m => KEYS.forall(k => fm(m).a.get(k).terms <= 1))
  // S3: スレッドごとに未裁定の項目は高々 1 つ。暗黙の supersede が働くのは、書き手が古い項目を
  // 見ていなかったときだけ(エージェントが move を先に書く規律が守られている)
  val s3 = MACH.forall(m =>
    not(fm(m).badImp) and
    Set(0, 1).forall(t => KEYS.filter(k => thr(k) == t and isUnres(stOf(m, k))).size() <= 1))
  // S4: stale の行は、書いた時点で「最後の非 remind のあとに、今日より前の日の remind がある」を満たす
  val s4 = MACH.forall(m => s.views.get(m).select(l => l.ev == "stale" and not(l.chk)).length() == 0)
  // S5: 重ねて届いた開く出来事(再読)は、畳み込みの状態を変えない
  val s5 = not(s.s5bad)
  // S6: 仕様が起こした遷移は、すべて語彙にある
  val s6 = s.fired.forall(t => TRANSITIONS.contains(t))
  // S7: 終端の行を消された項目を、台帳を持つ機体の check がちょうど拾う
  val s7 = MACH.forall(m =>
    flagged(m) == s.wrote.get(m).filter(k => not(hasTerm(s.views.get(m), k))))
  // S7: 拾った項目を、ふつうの未裁定の項目として remind しない
  val s7b = not(s.badRem)
  // 状態に持つ畳み込みの結果が、写しの行から畳み込み直した結果と一致する
  val cacheOk = MACH.forall(m => s.fo.get(m) == foldView(s.views.get(m)))
  // 語彙の状態が、仕様の状態と一致する
  val vocabStates = Set("awaiting", "decided", "closed", "dismissed", "stale", "superseded") == STATES

  // 語彙の遷移ごとの被覆(到達できなければ、語彙に死んだ遷移がある)。違反の反例が出れば到達可能
  def notFired(i: int): bool = i >= TRANSITIONS_LIST.length() or not(s.fired.contains(TRANSITIONS_LIST.nth(i)))
  val cov0 = notFired(0)
  val cov1 = notFired(1)
  val cov2 = notFired(2)
  val cov3 = notFired(3)
  val cov4 = notFired(4)
  val cov5 = notFired(5)
  val cov6 = notFired(6)
  val cov7 = notFired(7)
  val cov8 = notFired(8)
  val cov9 = notFired(9)
  val cov10 = notFired(10)
  val cov11 = notFired(11)
  val cov12 = notFired(12)
  val cov13 = notFired(13)
  val cov14 = notFired(14)
  val cov15 = notFired(15)
  val cov16 = notFired(16)
  val cov17 = notFired(17)
  val cov18 = notFired(18)
  val cov19 = notFired(19)

  // ---------------------------------------------------------------- 公平性と活性
  // 公平性は、日が進む・LiveSync が届く・掃引・エージェントの読み取り・台帳への追記に置く。
  // 本人の手(decide/pass/下書きの削除/送信/行の削除)と落ちる手には置かない。
  val vars = s
  temporal fair =
    weakFair(tick, vars) and
    MACH.forall(m =>
      weakFair(deliver(m), vars) and weakFair(createDraft(m), vars) and weakFair(recordLine(m), vars) and
      AKEYS.forall(k =>
        weakFair(agentOpen(m, k), vars) and weakFair(redraft(m, k), vars) and
        weakFair(agentApply(m, k), vars) and weakFair(ledgerAppend(m, k), vars) and
        ((SWEEP_ALL or m == 0) implies (weakFair(sweepRemind(m, k), vars) and weakFair(sweepStale(m, k), vars)))))

  def unresF(m: int, k: int): bool = isUnres(stOf(m, k)) and not(flagged(m).contains(k))
  // L1: 未裁定のまま残り続ける項目は無い(終端、stale、または台帳の控えと食い違う項目として拾われる)
  temporal l1at(m: int, k: int): bool = always(unresF(m, k) implies eventually(not(unresF(m, k))))
  temporal l1 = fair implies MACH.forall(m => (SWEEP_ALL or m == 0) implies AKEYS.forall(k => l1at(m, k)))
  // L2: 来た受信は、いずれ項目になるか、記録済みと見なされる(どの機体のエージェントも、もう開かない)
  temporal l2at(k: int): bool = always(s.rc.get(k).arr implies eventually(MACH.forall(m => handled(m, k))))
  temporal l2 = fair implies AKEYS.forall(k => l2at(k))
  // L3: 下書きの項目は、いずれ行が記録され、Gmail の下書きがある(作り直されたか、送られたか、消されたか)か、終端になる
  temporal l3at(k: int): bool = always(s.tried.contains(k) implies
    eventually(s.rec.contains(k) and (s.gm.get(k) != "none" or MACH.exists(m => isTerm(stOf(m, k))))))
  temporal l3 = fair implies AKEYS.forall(k => l3at(k))
}
