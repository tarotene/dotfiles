# Falcon Sensor は専用スクリプトで会社用 PC に導入する

CrowdStrike Falcon Sensor は root 権限と systemd サービスを使うため、system layer として管理する。会社から受け取った `.deb` と CID は Git に入れず、`company-pop-new` 上で専用スクリプトを実行する。

この runbook は `company-pop-new` の所有者向けで、ターミナルと sudo、そして SOPS 復号用の YubiKey が手元にあることを前提とする。初回導入では「IT 管理者の承認」から「端末登録」までを順に読み、導入後は更新と障害調査の節だけを参照すればよい。

## IT 管理者の承認を得てから始める

対象は Pop!_OS 24.04、x86_64 の `company-pop-new` に限る。CrowdStrike は Ubuntu 24.04 を Sensor 7.19.17219 以降の対応対象としているが、Pop!_OS は公式の対応一覧に明記されていない。本番導入の前に、次の点を社内 IT 管理者へ確認する。

- Pop!_OS 24.04 への導入が社内ポリシー上認められていること
- 配布された Linux 用 `.deb` のバージョンが指定どおりであること
- CID だけで登録でき、provisioning token、proxy、cloud region の指定が不要であること
- 端末から Falcon Cloud へ通信できること

## 配布物と CID はホスト内だけに置く

会社から受け取った `.deb` は Git 管理外に置く。`~/Downloads/` のままでよい。インストールスクリプトが root から読める作業ディレクトリへ複製してから apt に渡すので、保存場所に制約はない。リポジトリの作業ツリー内に置くと `git clean -xfd` で消えるため避ける。

CID はホストローカルの SOPS に入れる。新しいホストでは SOPS 自体が未初期化なので、`init` から始める。YubiKey を挿し、PIN を入力できる状態で実行する。

```bash
./scripts/setup-sops-secrets.sh init          # ~/.sops/.sops.yaml を作る（初回のみ）
./scripts/setup-sops-secrets.sh add-secret FALCON_CID
./scripts/setup-sops-secrets.sh validate
```

`init` を飛ばして `add-secret` を実行すると `Secrets file not found` で止まる。SOPS 全体の位置づけは [SETUP.md](../SETUP.md) と [ADR-0003](adr/0003-secrets-and-identity.md) を参照する。

`add-secret` は値を対話入力で受け取るので、シェル履歴には残らない。CID を `export FALCON_CID=...` と直接打つと履歴に平文で残るため、この経路は使わない。

インストールスクリプトは `FALCON_CID` が環境になければ SOPS から自力で読む。`exec zsh` や `reload_sops_secrets` でシェルへ読み込ませる必要はない。読み込めているかどうかは、次節の dry-run が報告する。

## dry-run の後にインストールする

最初に `--dry-run` で確認する。dry-run はホスト、OS、アーキテクチャ、`.deb` のメタデータを実際に検証し、CID が取得できるかどうかも報告する。飛ばすのはホストへ変更を加える処理だけである。

```bash
./scripts/install-falcon-sensor.sh \
  --dry-run \
  --package ~/Downloads/falcon-sensor_<version>_amd64.deb
```

表示された `.deb` のバージョンが IT 管理者の指定と一致していること、`FALCON_CID: [available]` になっていることを確認する。`[not available; ...]` なら前節の SOPS 設定に戻る。

内容を確認したら `--dry-run` を外して実行する。

```bash
./scripts/install-falcon-sensor.sh \
  --package ~/Downloads/falcon-sensor_<version>_amd64.deb
```

スクリプトは apt でパッケージを導入し、`falconctl` で CID を登録する。その後、`falcon-sensor.service` を有効化して起動する。CID はスクリプトの出力には表示しない。

導入の途中で、`.deb` の postinst が CID 設定前にサービスを起動しようとして一度失敗する。journal に `CID is not set. Use falconctl to set the CID` が 1 回残るのは正常で、スクリプトはこの失敗状態を消してから起動し直す。

## 端末登録まで確認して完了とする

ローカルでは、パッケージ、サービス、CID 登録を確認する。

```bash
dpkg-query -W -f='${Status} ${Version}\n' falcon-sensor
systemctl is-enabled falcon-sensor.service
systemctl is-active falcon-sensor.service
sudo /opt/CrowdStrike/falconctl -g --cid >/dev/null && echo "CID is set"
```

`falconctl -g --cid` は CID を標準出力へ書くので、端末とスクロールバックに平文で残さないよう捨てて終了ステータスだけを見る。

プロセス名での確認はしない。`falcon-sensor.service` が起動するのは `ExecStart=/opt/CrowdStrike/falcond` で、稼働中に見えるのは `falcond` とその子の `falcon-sensor-bpf` である。`falcon-sensor` という名前のプロセスは存在しないので、`pgrep -x falcon-sensor` は必ず失敗する。unit は `Type=forking` で PIDFile を持つため、`is-active` が `falcond` の生存まで見ている。

Falcon Console への登録まで確かめたい場合は AID を見る。CID とは違い、AID はセンサーが Falcon Cloud と通信できて初めて埋まる。

```bash
sudo /opt/CrowdStrike/falconctl -g --aid >/dev/null && echo "AID is set"
```

`/opt/CrowdStrike` は `0750 root:root` なので、この配下は `sudo` なしでは存在確認すらできない。`ls /opt/CrowdStrike` が `Permission denied` になるのは正常で、導入失敗の兆候ではない。

最後に、社内 IT 管理者へ Falcon Console の Newly Installed Sensors に `company-pop-new` が現れたことを確認してもらう。ローカルサービスが動いていても、Console に現れなければ導入完了とはしない。

## 更新にも同じスクリプトを使う

社内 IT 管理者から新しい `.deb` を受け取ったら、同じ手順で dry-run の後に再実行する。スクリプトは CID を再設定し、サービスが有効かつ稼働中であることを確認する。

同じことが失敗からの復旧にも当てはまる。スクリプトの各ステップは冪等で、パッケージだけ入ってサービスが failed のような中途半端な状態からでも、同じコマンドをもう一度実行すれば正常な状態へ収束する。まず再実行を試す。

アンインストール、CID の変更、proxy、sensor tag、provisioning token の追加は、このスクリプトでは扱わない。再実行でも直らない場合は自動削除はせず、次の情報を添えて社内 IT 管理者へ連絡する。

```bash
sudo systemctl status falcon-sensor --no-pager
sudo journalctl -u falcon-sensor -n 50 --no-pager
dpkg-query -W falcon-sensor
```

## 用語と公式資料

- **Falcon Sensor**: 端末上で常駐する CrowdStrike のセンサー。本リポジトリでは `falcon-sensor.service` として扱う。
- **CID**: 端末を会社の Falcon テナントへ関連付ける Customer ID。ホストローカルの SOPS に保存する。
- **SOPS**: CID などを暗号化して保存し、必要なときに復号して読み込む既存の秘密管理手段。復号には YubiKey が必要。
- **falconctl**: `/opt/CrowdStrike/falconctl`。CID の設定と参照に使う管理コマンド。root 専用。
- **falcond**: `/opt/CrowdStrike/falcond`。`falcon-sensor.service` の `ExecStart` が起動する常駐プロセスの実体。子プロセスとして `falcon-sensor-bpf` が動く。
- **AID**: Agent ID。センサーが Falcon Cloud と通信できたときに割り当てられる端末固有の識別子。CID を設定しただけでは埋まらない。

対応 OS と最低センサーバージョンは [CrowdStrike の展開 FAQ](https://www.crowdstrike.com/ja-jp/products/faq/)、基本的な Linux 導入手順は [Installing Falcon Sensor for Linux](https://www.crowdstrike.com/tech-hub/endpoint-security/installing-falcon-sensor-for-linux/) を参照する。この runbook で解決しない問題やポリシー判断は、作業を止めて社内 IT 管理者へ問い合わせる。

最終更新: 2026-07-28
