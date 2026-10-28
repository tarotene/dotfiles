// 個人と勤務先のカレンダーの握手プロトコルの仕様(Quint)。
//
// 読み方は docs/agents/calendar-handshake.md の「仕様と検査」の節にある。
// このファイルは論理だけを持つ。次の 2 つは検査のたびに生成する(コミットしない):
//   - ./vocab.qnt  vocabulary.json から jq で生成する。遷移の可否の正本は語彙 JSON。
//   - ./cfg.qnt    scenarios.json の 1 件から生成する。規則の有効・無効と環境の絞り込み。
//
// 参加者:
//   本人(person)        題名の印を任意の値に書き換えられる(スマホから)。予定の削除もする。
//   会社側 script        10 分ごと。1 回の実行は複数の書き込みに分かれ、途中で落ち、重複して動く。
//   会社 PC のエージェント [blocked] を [secured] にする(本人が送ったあとだけ)。
//   朝の机(desk)         停滞を警告する。
// 共有記憶:
//   evs   専用カレンダー上の予定(印・題名の残り・日・組の id・最後に受理した印)
//   abs   会社カレンダー上の不在(印の予定 id と日を持つ)
//
// 前提(外部の挙動。一次情報は ADR の「前提」の節):
//   A1  events.update/patch は ETag を If-Match に付けると、変更されていれば 412 で失敗する。
//       script の書き込みは「読んだときから予定が変わっていなければ成功」(CAS)として扱う。
//   A2  LockService のロックは、スクリプトの終了(異常終了を含む)で解放される。
//   A3  orphanScan(会社側の不在の掃除)は 1 ステップで終わる。実際は複数回の API 呼び出しで、
//       この前提は楽観的である(仕様の限界として ADR に書く)。
module handshake {
  import vocab.* from "./vocab"
  import cfg.* from "./cfg"

  // ---------------------------------------------------------------- 型
  type Mark = Hold | Ok | Resched | Ng | Go | Blocked | Secured
  type Phase = Idle | Read | Looked | Created | Pruned | Reset
  type Ev = {
    present: bool,
    mark: Mark,
    title: str,   // 印を除いた題名の残り
    start: int,   // 開始日(抽象値)
    gid: str,     // script が最初の判定で刻む組の id(空 = まだ刻んでいない)
    ack: Mark,    // script が最後に受理した印(拡張プロパティ)
    sent: bool,   // 本人が連絡・申請を送った
    intent: str,  // ゴースト: 本人がどの会議の候補として作ったか。script からは見えない
  }
  type Run = { ph: Phase, e: int, snap: Ev, found: bool }
  type Abs = { ev: int, start: int }

  // ---------------------------------------------------------------- 定数
  pure val IDS = Set(0, 1)
  pure val RUN_IDS = if (TWO_RUNS) Set(0, 1) else Set(0)
  pure val TITLES = Set("A", "B")
  pure val STARTS = Set(2, 3)
  pure val MAX_DAY = 3
  pure val WARN_BEFORE = 2
  pure val MAX_ABS = 3
  pure val MARKS = Set(Hold, Ok, Resched, Ng, Go, Blocked, Secured)

  pure val noEv: Ev = {
    present: false, mark: Hold, title: "", start: 0, gid: "", ack: Hold, sent: false, intent: "",
  }
  pure val idleRun: Run = { ph: Idle, e: 0, snap: noEv, found: false }
  pure val noTr: Tr = { from: "", to: "", by: "" }

  // ---------------------------------------------------------------- 状態
  var evs: int -> Ev
  var abs: List[Abs]
  var runs: int -> Run
  var lock: int                        // ロックを持つ run(-1 = 無し)
  var env: { crashes: int, edits: int, today: int, warned: Set[int] }
  var fired: Tr                        // ゴースト: 直前に起きた遷移(語彙との照合用)

  // ---------------------------------------------------------------- 語彙との対応
  pure def sid(m: Mark): str = match m {
    | Hold => "candidate"
    | Ok => "ok"
    | Resched => "resched"
    | Ng => "ng"
    | Go => "go"
    | Blocked => "blocked"
    | Secured => "secured"
  }
  pure def legalStep(a: Mark, b: Mark): bool =
    TRANSITIONS.exists(t => t.from == sid(a) and t.to == sid(b))

  pure def isHard(m: Mark): bool = m == Blocked or m == Secured
  pure def isSoft(m: Mark): bool = not(isHard(m))
  // 組のキー: 刻んだ id があればそれ、無ければ題名の残り(ADR-550 D1 のまま)
  pure def gkey(ev: Ev): str = if (STAMP_GROUP and ev.gid != "") ev.gid else ev.title
  pure def stampOf(ev: Ev): str = if (STAMP_GROUP and ev.gid == "") ev.title else ev.gid

  def absFor(e: int): List[Abs] = abs.select(a => a.ev == e)
  def consistent(e: int): bool =
    absFor(e).length() == 1 and absFor(e).head().start == evs.get(e).start
  // e 以外の同じ組に、確保済み([blocked]/[secured])の予定がある
  def closedGroup(e: int): bool =
    IDS.exists(o => o != e and evs.get(o).present and gkey(evs.get(o)) == gkey(evs.get(e))
                    and isHard(evs.get(o).mark))
  def isOrphan(a: Abs): bool =
    not(evs.get(a.ev).present) or
    (CANCEL_PROPAGATES and (evs.get(a.ev).mark == Hold or evs.get(a.ev).mark == Ok
                            or evs.get(a.ev).mark == Resched or evs.get(a.ev).mark == Ng))

  // ---------------------------------------------------------------- 初期状態
  action init = all {
    evs' = IDS.mapBy(i => noEv),
    abs' = [],
    runs' = Set(0, 1).mapBy(r => idleRun),   // 使わない run は RUN_IDS で起動を禁じる
    lock' = -1,
    env' = { crashes: 0, edits: 0, today: 0, warned: Set() },
    fired' = noTr,
  }

  // ---------------------------------------------------------------- 補助の action
  action keepRuns = all { runs' = runs, lock' = lock }
  action keepAbs = abs' = abs

  def sn(r: int): Ev = runs.get(r).snap
  def ee(r: int): int = runs.get(r).e
  def casOk(r: int): bool = evs.get(ee(r)) == sn(r)

  // 書き込みなしで run を終える(ロックを手放す)
  action endRun(r: int): bool = all {
    runs' = runs.set(r, idleRun),
    lock' = if (lock == r) -1 else lock,
    evs' = evs, abs' = abs, env' = env, fired' = fired,
  }

  // 読んだときから予定が変わっていなければ書いて終わる(A1)。変わっていれば何も書かずに終わる
  action casWrite(r: int, newEv: Ev, tr: Tr): bool = all {
    runs' = runs.set(r, idleRun),
    lock' = if (lock == r) -1 else lock,
    evs' = if (casOk(r)) evs.set(ee(r), newEv) else evs,
    fired' = if (casOk(r)) tr else fired,
    abs' = abs, env' = env,
  }

  // ---------------------------------------------------------------- script
  // 1 回の実行は、1 つの予定を読むところから始まる。LOCKED なら実行全体をロックで囲む。
  action runStart(r: int, e: int): bool = all {
    RUN_IDS.contains(r),
    runs.get(r).ph == Idle,
    evs.get(e).present,
    LOCKED implies lock == -1,
    runs' = runs.set(r, { ph: Read, e: e, snap: evs.get(e), found: false }),
    lock' = if (LOCKED) r else lock,
    evs' = evs, abs' = abs, env' = env, fired' = fired,
  }

  def readPhase(r: int): bool = runs.get(r).ph == Read
  // 許されない書き換え(語彙の遷移に無い)を見つけた / 許される書き換えを見つけた
  def plain(r: int): bool = not(REVERT) or sn(r).ack == sn(r).mark
  def illegalSeen(r: int): bool =
    REVERT and sn(r).ack != sn(r).mark and not(legalStep(sn(r).ack, sn(r).mark))
  def legalSeen(r: int): bool =
    REVERT and sn(r).ack != sn(r).mark and legalStep(sn(r).ack, sn(r).mark)

  // S6: 許されない書き換えは、最後に受理した印へ差し戻す
  action doRevert(r: int): bool = all {
    readPhase(r), illegalSeen(r),
    casWrite(r, { ...sn(r), mark: sn(r).ack }, fired),
  }
  // 許される書き換え(別の主体の正規の遷移)は、受理済みとして記録する
  action doAck(r: int): bool = all {
    readPhase(r), legalSeen(r),
    casWrite(r, { ...sn(r), ack: sn(r).mark }, fired),
  }

  // [hold] の判定。閉じた組への新しい候補は [ng](H6)
  action doJudge(r: int): bool = {
    nondet j0 = Set(Ok, Resched, Ng).oneOf()
    val j = if (CLOSE_GROUP and closedGroup(ee(r))) Ng else j0
    all {
      readPhase(r), plain(r), sn(r).mark == Hold,
      casWrite(r, { ...sn(r), mark: j, ack: j, gid: stampOf(sn(r)) },
               { from: "candidate", to: sid(j), by: "workplace-script" }),
    }
  }

  // [ok]/[resched]/[ng] は script からは何もしない
  action softEnd(r: int): bool = all {
    readPhase(r), plain(r),
    sn(r).mark == Ok or sn(r).mark == Resched or sn(r).mark == Ng,
    endRun(r),
  }

  // [go]: 不在が既にあるかを探す。
  // 閉じた組への [go] を [ng] にする遷移は置かない: 判定の時点の閉じ込め(doJudge)と、
  // [go] の処理の候補削除(goPrune)が先に効き、[go] の時点で組が閉じていることが無い
  // (落ちる実行・2 つの実行・題名の編集・削除・取り下げ・日の編集を入れた広い環境でも
  // 到達不能であることを TLC で確かめた。ADR の「検査の結果」)。
  action goLookup(r: int): bool = all {
    readPhase(r), plain(r), sn(r).mark == Go,
    runs' = runs.set(r, { ...runs.get(r), ph: Looked, found: absFor(ee(r)).length() > 0 }),
    lock' = lock, evs' = evs, abs' = abs, env' = env, fired' = fired,
  }
  // [go]: 不在を入れる(冪等なら、既にあれば入れない)
  action goCreate(r: int): bool = all {
    runs.get(r).ph == Looked,
    abs' = if ((IDEMPOTENT_ABSENCE and runs.get(r).found) or abs.length() >= MAX_ABS) abs
           else abs.append({ ev: ee(r), start: sn(r).start }),
    runs' = runs.set(r, { ...runs.get(r), ph: Created }),
    lock' = lock, evs' = evs, env' = env, fired' = fired,
  }
  // [go]: 同じ組の他の候補を削除する
  action goPrune(r: int): bool = all {
    runs.get(r).ph == Created,
    evs' = IDS.mapBy(o =>
      if (o != ee(r) and evs.get(o).present and gkey(evs.get(o)) == gkey(sn(r))
          and isSoft(evs.get(o).mark)) { ...evs.get(o), present: false }
      else evs.get(o)),
    runs' = runs.set(r, { ...runs.get(r), ph: Pruned }),
    lock' = lock, abs' = abs, env' = env, fired' = fired,
  }
  // [go]: 印を [blocked] に直す(確定の点)
  action goBlock(r: int): bool = all {
    runs.get(r).ph == Pruned,
    casWrite(r, { ...sn(r), mark: Blocked, ack: Blocked },
             { from: "go", to: "blocked", by: "workplace-script" }),
  }

  // [blocked]/[secured]: 不在と整合していれば何もしない。食い違えば不在を消して [go] に戻す(H3・H5)
  action hardOk(r: int): bool = all {
    readPhase(r), plain(r), isHard(sn(r).mark),
    not(RECONCILE) or consistent(ee(r)),
    endRun(r),
  }
  action hardReset(r: int): bool = all {
    readPhase(r), plain(r), isHard(sn(r).mark),
    RECONCILE, not(consistent(ee(r))),
    abs' = abs.select(a => a.ev != ee(r)),
    runs' = runs.set(r, { ...runs.get(r), ph: Reset }),
    lock' = lock, evs' = evs, env' = env, fired' = fired,
  }
  action resetWrite(r: int): bool = all {
    runs.get(r).ph == Reset,
    casWrite(r, { ...sn(r), mark: Go, ack: Go, sent: false },
             { from: sid(sn(r).mark), to: "go", by: "workplace-script" }),
  }

  // 孤児の不在の掃除(S4)。本人が印を消した・戻したあとの取り消しの伝播
  action orphanScan(r: int): bool = all {
    RUN_IDS.contains(r),
    runs.get(r).ph == Idle,
    LOCKED implies lock == -1,
    abs.select(a => isOrphan(a)).length() > 0,
    abs' = abs.select(a => not(isOrphan(a))),
    runs' = runs, lock' = lock, evs' = evs, env' = env, fired' = fired,
  }

  // script が途中で落ちる(A2: ロックは解放される)
  action runCrash(r: int): bool = all {
    CAN_CRASH,
    runs.get(r).ph != Idle,
    env.crashes < MAX_CRASHES,
    runs' = runs.set(r, idleRun),
    lock' = if (lock == r) -1 else lock,
    env' = { ...env, crashes: env.crashes + 1 },
    evs' = evs, abs' = abs, fired' = fired,
  }

  // ---------------------------------------------------------------- 本人
  val budget = env.edits < MAX_EDITS
  action spend = env' = { ...env, edits: env.edits + 1 }

  action personCreate(e: int, t: str, s: int): bool = all {
    budget, not(evs.get(e).present),
    evs' = evs.set(e, { present: true, mark: Hold, title: t, start: s, gid: "", ack: Hold,
                        sent: false, intent: t }),
    spend, keepRuns, keepAbs, fired' = fired,
  }
  // 正規の遷移: [ok]/[resched] → [go]
  action personGo(e: int): bool = all {
    budget, evs.get(e).present, evs.get(e).mark == Ok or evs.get(e).mark == Resched,
    evs' = evs.set(e, { ...evs.get(e), mark: Go }),
    fired' = { from: sid(evs.get(e).mark), to: "go", by: "person" },
    spend, keepRuns, keepAbs,
  }
  // 取り下げ・やり直し(語彙の遷移): どの印からでも [hold] に戻す
  action personCancel(e: int): bool = all {
    CAN_CANCEL, budget, evs.get(e).present, evs.get(e).mark != Hold,
    evs' = evs.set(e, { ...evs.get(e), mark: Hold, sent: false }),
    fired' = { from: sid(evs.get(e).mark), to: "candidate", by: "person" },
    spend, keepRuns, keepAbs,
  }
  action personEditTitle(e: int, t: str): bool = all {
    CAN_EDIT_TITLE, budget, evs.get(e).present, evs.get(e).title != t,
    // 環境の仮定 E1: 題名の編集は、最初の判定のあと(script が組の id を刻んだあと)に起きる
    not(EDIT_AFTER_JUDGED) or evs.get(e).mark != Hold,
    evs' = evs.set(e, { ...evs.get(e), title: t }),
    spend, keepRuns, keepAbs, fired' = fired,
  }
  action personEditStart(e: int, s: int): bool = all {
    CAN_EDIT_START, budget, evs.get(e).present, evs.get(e).start != s,
    evs' = evs.set(e, { ...evs.get(e), start: s }),
    spend, keepRuns, keepAbs, fired' = fired,
  }
  // 語彙に無い書き換えを含む、任意の印の書き込み(スマホからの手書き)
  action personForge(e: int, m: Mark): bool = all {
    CAN_FORGE, budget, evs.get(e).present, evs.get(e).mark != m,
    evs' = evs.set(e, { ...evs.get(e), mark: m }),
    spend, keepRuns, keepAbs, fired' = fired,
  }
  action personDelete(e: int): bool = all {
    CAN_DELETE, budget, evs.get(e).present,
    evs' = evs.set(e, { ...evs.get(e), present: false }),
    spend, keepRuns, keepAbs, fired' = fired,
  }
  // 連絡・申請を送る(書き込み回数には数えない)
  action personSend(e: int): bool = all {
    evs.get(e).present, evs.get(e).mark == Blocked, not(evs.get(e).sent),
    evs' = evs.set(e, { ...evs.get(e), sent: true }),
    env' = env, keepRuns, keepAbs, fired' = fired,
  }

  // ---------------------------------------------------------------- 会社 PC のエージェント
  action agentSecure(e: int): bool = all {
    evs.get(e).present, evs.get(e).mark == Blocked, evs.get(e).sent,
    evs' = evs.set(e, { ...evs.get(e), mark: Secured }),
    fired' = { from: "blocked", to: "secured", by: "workplace-agent" },
    env' = env, keepRuns, keepAbs,
  }

  // ---------------------------------------------------------------- 朝の机
  def stalled(e: int): bool =
    evs.get(e).present and (evs.get(e).mark == Go or evs.get(e).mark == Blocked)
    and evs.get(e).start - env.today <= WARN_BEFORE
    and not(IDS.exists(o => o != e and evs.get(o).present and gkey(evs.get(o)) == gkey(evs.get(e))
                            and evs.get(o).mark == Secured))
  val stalledSet = IDS.filter(e => stalled(e))
  action deskRun: bool = all {
    WITH_DESK,
    stalledSet.exclude(env.warned).size() > 0,
    env' = { ...env, warned: env.warned.union(stalledSet) },
    evs' = evs, keepRuns, keepAbs, fired' = fired,
  }
  action tick: bool = all {
    WITH_DESK, env.today < MAX_DAY,
    env' = { ...env, today: env.today + 1 },
    evs' = evs, keepRuns, keepAbs, fired' = fired,
  }

  // 何も起きない遷移(全員が止まった状態を行き止まりにしない。活性は公平性の仮定のもとで見る)
  action stutter = all {
    evs' = evs, abs' = abs, runs' = runs, lock' = lock, env' = env, fired' = fired,
  }

  // 分岐ごとに必要な nondet だけを持たせる(使わない引数の直積を TLC に列挙させない)
  action stepStart = {
    nondet r = RUN_IDS.oneOf()
    nondet e = IDS.oneOf()
    runStart(r, e)
  }
  action stepRun = {
    nondet r = RUN_IDS.oneOf()
    any {
      doRevert(r), doAck(r), doJudge(r), softEnd(r),
      goLookup(r), goCreate(r), goPrune(r), goBlock(r),
      hardOk(r), hardReset(r), resetWrite(r), orphanScan(r), runCrash(r),
    }
  }
  action stepCreate = {
    nondet e = IDS.oneOf()
    nondet t = TITLES.oneOf()
    nondet s = STARTS.oneOf()
    personCreate(e, t, s)
  }
  action stepTitle = {
    nondet e = IDS.oneOf()
    nondet t = TITLES.oneOf()
    personEditTitle(e, t)
  }
  action stepStartDay = {
    nondet e = IDS.oneOf()
    nondet s = STARTS.oneOf()
    personEditStart(e, s)
  }
  action stepForge = {
    nondet e = IDS.oneOf()
    nondet m = MARKS.oneOf()
    personForge(e, m)
  }
  action stepPerson = {
    nondet e = IDS.oneOf()
    any { personGo(e), personCancel(e), personDelete(e), personSend(e), agentSecure(e) }
  }

  action step = any {
    stepStart, stepRun, stepCreate, stepTitle, stepStartDay, stepForge, stepPerson,
    deskRun, tick, stutter,
  }

  // ================================================================ 性質
  def intentHard(i: str): Set[int] =
    IDS.filter(e => evs.get(e).present and evs.get(e).intent == i and isHard(evs.get(e).mark))

  // S1: 同じ会議の候補で、[blocked]・[secured] は高々 1 件
  val s1 = TITLES.forall(i => intentHard(i).size() <= 1)
  // S2: 不在は二重に入らない
  val s2dup = IDS.forall(e => absFor(e).length() <= 1)
  // S2: [blocked]・[secured] には、日の合った不在がちょうど 1 件ある(本人の書き換えが無い環境で)
  val s2blk = IDS.forall(e =>
    (evs.get(e).present and isHard(evs.get(e).mark)) implies consistent(e))
  // S3: [blocked]・[secured] のあと、同じ会議の他の候補が(判定待ちの [hold] と [ng] の他に)残らない
  val s3 = IDS.forall(e =>
    (evs.get(e).present and isHard(evs.get(e).mark)) implies
      IDS.forall(o => (o != e and evs.get(o).present and evs.get(o).intent == evs.get(e).intent) implies
        (evs.get(o).mark == Hold or evs.get(o).mark == Ng)))
  // S6: 仕様が起こした遷移は、すべて語彙の遷移にある
  val s6 = fired.from == "" or TRANSITIONS.contains(fired)
  // 語彙の状態が仕様の印と一致する
  val vocabStates = MARKS.map(m => sid(m)) == STATES

  // 語彙の遷移ごとの被覆(到達できなければ、語彙に死んだ遷移がある)。違反の反例が出れば到達可能
  def notFired(i: int): bool = i >= TRANSITIONS_LIST.length() or fired != TRANSITIONS_LIST.nth(i)
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
  val vars = (evs, abs, runs, lock, env, fired)
  temporal fair =
    strongFair(runStart(0, 0), vars) and strongFair(runStart(0, 1), vars) and
    strongFair(runStart(1, 0), vars) and strongFair(runStart(1, 1), vars) and
    weakFair(goLookup(0), vars) and weakFair(goCreate(0), vars) and weakFair(goPrune(0), vars) and
    weakFair(goBlock(0), vars) and weakFair(resetWrite(0), vars) and weakFair(hardReset(0), vars) and
    weakFair(doJudge(0), vars) and weakFair(doRevert(0), vars) and weakFair(doAck(0), vars) and
    weakFair(softEnd(0), vars) and weakFair(hardOk(0), vars) and
    weakFair(goLookup(1), vars) and weakFair(goCreate(1), vars) and weakFair(goPrune(1), vars) and
    weakFair(goBlock(1), vars) and weakFair(resetWrite(1), vars) and weakFair(hardReset(1), vars) and
    weakFair(doJudge(1), vars) and weakFair(doRevert(1), vars) and weakFair(doAck(1), vars) and
    weakFair(softEnd(1), vars) and weakFair(hardOk(1), vars) and
    strongFair(orphanScan(0), vars) and strongFair(orphanScan(1), vars) and
    weakFair(agentSecure(0), vars) and weakFair(agentSecure(1), vars) and
    weakFair(deskRun, vars)

  // L1: [go] は、いずれ [go] でなくなる([blocked]、閉じた組への [ng]、または本人が動かした)
  temporal l1at(e: int): bool = always((evs.get(e).present and evs.get(e).mark == Go) implies
                                  eventually(not(evs.get(e).present) or evs.get(e).mark != Go))
  temporal l1 = fair implies (l1at(0) and l1at(1))
  // L2: 送ったあとの [blocked] は、いずれ [blocked] でなくなる
  temporal l2at(e: int): bool = always((evs.get(e).present and evs.get(e).mark == Blocked and evs.get(e).sent) implies
                                  eventually(not(evs.get(e).present) or evs.get(e).mark != Blocked))
  temporal l2 = fair implies (l2at(0) and l2at(1))
  // L3: 停滞し続ける予定は、いずれ朝の机の警告に達する。
  // 「一瞬だけ停滞して机が動く前に解消した」予定には警告を出せず、出す必要もない(反例で確認した)
  temporal l3at(e: int): bool = always(always(stalled(e)) implies eventually(env.warned.contains(e)))
  temporal l3 = fair implies (l3at(0) and l3at(1))
  // S4: 孤児の不在は、いずれ消える
  // 孤児の定義は規則に依らない: 予定が消えた、または印が [hold]/[ok]/[resched]/[ng] に戻った不在
  def isStale(a: Abs): bool =
    not(evs.get(a.ev).present) or
    (evs.get(a.ev).mark == Hold or evs.get(a.ev).mark == Ok
     or evs.get(a.ev).mark == Resched or evs.get(a.ev).mark == Ng)
  temporal s4 = fair implies always(abs.select(a => isStale(a)).length() > 0 implies
                                    eventually(abs.select(a => isStale(a)).length() == 0))
  // S2(本人の書き換えがある環境): [blocked]・[secured] と不在の食い違いは、いずれ直る
  temporal s2healAt(e: int): bool = always((evs.get(e).present and isHard(evs.get(e).mark) and not(consistent(e))) implies
                                      eventually(not(evs.get(e).present) or not(isHard(evs.get(e).mark)) or consistent(e)))
  temporal s2heal = fair implies (s2healAt(0) and s2healAt(1))
  // S6(応答): 許されない書き換えは、いずれ差し戻される
  def illegalMark(e: int): bool =
    evs.get(e).present and evs.get(e).ack != evs.get(e).mark and not(legalStep(evs.get(e).ack, evs.get(e).mark))
  temporal s6revertAt(e: int): bool = always(illegalMark(e) implies eventually(not(illegalMark(e))))
  temporal s6revert = fair implies (s6revertAt(0) and s6revertAt(1))
}
