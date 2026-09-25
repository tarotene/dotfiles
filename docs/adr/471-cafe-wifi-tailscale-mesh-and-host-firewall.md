# ADR-471 — カフェ Wi-Fi 対策: Tailscale メッシュ + Mullvad 出口 + ホスト firewall

- Status: Accepted
- Date: 2026-09-25
- Issue: No-Issue(grill-me セッションで裁定)

## Context

この dotfiles リポジトリには VPN・DNS・ホスト firewall に関する設定・決定が
一切存在しない。一方でユーザーはカフェ等の公衆 Wi-Fi を日常的に使う。実際に
残るリスクは次の 2 つに大別される:

- (a) 同一 LAN 上の他端末からの到達(SSH・Syncthing 等の待受ポート、mDNS)
- (b) カフェ運営者/ISP による DNS・SNI・接続先メタデータの観測

対象端末は本リポジトリが管理する Linux 2 台(`vega` = personal、`arcturus` =
company)・darwin 1 台(`altair` = personal)に加え、公式クライアントが揃う
スマートフォンも含む。

自宅拠点は現時点で常時稼働機が無いが、中長期でラズパイ/Kubernetes スタックを
運用する計画がある。これは Obsidian が既に引いている二段構え(今は Cloudflare
R2 上の Self-hosted LiveSync、中長期で自宅スタックへローカル化 —
`docs/operations.md` "Obsidian vault backup" 節)と同型であり、ネットワーク側も
同じ形に揃えることで、ラズパイスタックが立った時点の切替コストを最小化できる。

## Decision

メッシュ VPN として **Tailscale** を採用する。制御プレーンは今は Tailscale の
SaaS(Personal プラン、無料)、中長期で自宅スタック上の **Headscale** に移行する
— どちらも Tailscale の公式クライアント(Linux/macOS/iOS/Android)がそのまま
使えるため、切替は「ログイン先 URL を変えて再ログイン」だけで済む。

自宅拠点が立つまでの「信頼できる出口」は Tailscale の **Mullvad 出口ノード
add-on**(月 $5・5 台まで)を使う。中長期でラズパイを `--advertise-exit-node`
した自宅出口に切り替える。

ホスト firewall は場所(カフェ/自宅)を判定して切り替える仕組みを持たず、常時
同じ 1 ルールセットを適用する:

- Linux: 入方向既定拒否、`tailscale0` からのみ許可、Tailscale の直結用
  41641/udp を許可(`ufw`)。
- darwin: アプリケーション firewall(ALF)の Block all incoming connections
  + stealth mode。ALF はインターフェース単位の許可を表現できないため、
  Linux の「tailnet からのみ許可」ではなく「全入方向拒否」を境界にする。
  Tailscale SSH server は Tailscale.app 内の userspace netstack で着信を
  受けるため、ALF の block-all の影響を受けない。

信頼境界は Tailscale の ACL policy(`config/tailscale/policy.hujson`)で宣言する:
`tag:company`(会社 PC `arcturus`)は `autogroup:internet`(Mullvad 出口)のみ
到達可で、個人端末とは相互に不到達。個人端末(`autogroup:member`)同士は
互接続と出口利用の両方を許可する。

プリンタは固定 IP(DHCP 予約)+ driverless IPP(`ipp://<ip>/ipp/print`)で
登録し、mDNS 発見への firewall 例外は開けない。

## クラウド → ローカルの二段構え

Obsidian と対にすると次の表になる。

| 対象 | 今(クラウド段階) | 中長期(自宅拠点段階) | 切替の実体 |
|---|---|---|---|
| Obsidian 同期 | Cloudflare R2 上の Self-hosted LiveSync(既存、変更なし) | 自宅ラズパイ/K8s 上の CouchDB(tailnet 内、LiveSync 作者の Tailscale Serve 構成) | LiveSync の接続先 URL を変える(このリポジトリでは変更しない) |
| メッシュ VPN 制御プレーン | Tailscale SaaS(Personal, 無料) | Headscale(ラズパイ) | 各端末で `--login-server` を変えて再ログイン |
| 信頼できる出口 | Tailscale の Mullvad 出口ノード add-on | ラズパイを `--advertise-exit-node` | prefs の `exit_node` 値を変える + add-on を解約 |

## Alternatives considered

メッシュ VPN(D1、本命は憧れ駆動): 対抗馬に NetBird(公式モバイルクライアント、
upstream 公式の self-host、routing peer による出口)を立てた。NetBird は商用 VPN
出口パートナーを持たず、自宅拠点か VPS を立てるまでカフェ対策が成立しない点で
劣る。鍵配布を自前で行う手組みトンネル構成・ZeroTier は感触で外した(スマホの鍵配布・NAT 越えを
自前で持つ気がしない)— 分析ではない。

暫定出口(D2): 対抗馬に小さな VPS を Tailscale 出口ノードにする案を立てた
(同じ「クライアント 1 つ」軸を満たすが、運用対象が 1 台増え、出口 IP が本人 1
人に紐付く)。Mullvad アプリ / Proton VPN の併用は感触で外した(iOS/Android は
VPN を同時に 1 本しか張れないため)。

詳細な出典・取得日は `grill-me` セッションのプランファイルの `## 先行例との
対比` 節(D1–D7)を参照。主な一次情報: Tailscale KB の各記事(free-plans,
mullvad-exit-nodes, other-vpns, exit-nodes, dns, tailnet を守るベスト
プラクティス集[KB #1196], firewall-ports, acl-syntax, tags, auto-exit-nodes,
macos-variants)、Headscale docs(clients, faq)、NetBird docs、OpenPrinting
CUPS docs、Apple Support(いずれも取得 2026-09-25)。

## Consequences

- Mullvad 出口ノード add-on は撮取日時点で "beta" 表記。自宅ラズパイ出口へ
  移行した時点で解約する(撤収条件)。
- Headscale FAQ は「headscale を tailnet 内のノードで動かすと subnet
  router・MagicDNS が壊れうる」と明記する。ラズパイ移行時は headscale 自身を
  tailnet 外(または別扱いのノード)として構成する必要がある。
- 会社 PC(`arcturus`)を個人 tailnet に参加させることは会社のセキュリティ
  ポリシー上の可否をユーザー自身が確認する前提とする(このリポジトリは
  ACL でその参加を「出口利用のみ」に縮小するところまでを担保する)。
- darwin の LAN 到達遮断は Linux と非対称(全入方向拒否 vs tailnet からのみ
  許可)。macOS で tailnet 限定の待受サービスが将来必要になったら pf への
  移行を再検討する。

## 執行点

- `packages/declarative/apt-packages.txt` — Tailscale(daemon)と ufw を
  system-layer パッケージとして追加
- `scripts/install-packages.sh` — Tailscale apt repo/keyring の冪等セットアップ
  + `tailscale up --operator`
- `packages/declarative/Brewfile` — darwin 用 `tailscale-app` cask
- `scripts/setup-firewall.sh` — 新規。Linux(ufw)/ darwin(ALF)のホスト
  firewall escape-hatch スクリプト
- `.github/workflows/ci.yml` — `setup-firewall.sh --dry-run` を dry-run job に追加
- `docs/operations.md` — "Café Wi-Fi" 運用節を新設
