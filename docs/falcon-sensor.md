# Falcon Sensor は専用スクリプトで会社用 PC に導入する

CrowdStrike Falcon Sensor は root 権限と systemd サービスを使うため、system layer として管理する。会社から受け取った `.deb` と CID は Git に入れず、`company-pop-new` 上で専用スクリプトを実行する。

この runbook は `company-pop-new` の所有者向けで、ターミナル、sudo、既存の SOPS を使えることを前提とする。初回導入では「IT 管理者の承認」から「端末登録」までを順に読み、導入後は更新と障害調査の節だけを参照すればよい。

## IT 管理者の承認を得てから始める

対象は Pop!_OS 24.04、x86_64 の `company-pop-new` に限る。CrowdStrike は Ubuntu 24.04 を Sensor 7.19.17219 以降の対応対象としているが、Pop!_OS は公式の対応一覧に明記されていない。本番導入の前に、次の点を社内 IT 管理者へ確認する。

- Pop!_OS 24.04 への導入が社内ポリシー上認められていること
- 配布された Linux 用 `.deb` のバージョンが指定どおりであること
- CID だけで登録でき、provisioning token、proxy、cloud region の指定が不要であること
- 端末から Falcon Cloud へ通信できること

## 配布物と CID はホスト内だけに置く

会社から受け取った `.deb` は、Git 管理外の `local/installers/` に保存する。

```bash
mkdir -p local/installers
mv ~/Downloads/falcon-sensor_*.deb local/installers/
```

CID は既存のホストローカル SOPS に追加する。

```bash
./scripts/setup-sops-secrets.sh add-secret FALCON_CID
exec zsh
```

`reload_sops_secrets` を使って現在のシェルへ読み直してもよい。値を表示せずに読み込みを確認するには、次を実行する。

```bash
[[ -n "${FALCON_CID:-}" ]] && echo "FALCON_CID is set"
```

## dry-run の後にインストールする

最初に、実行予定の確認項目と処理だけを表示する。

```bash
./scripts/install-falcon-sensor.sh \
  --dry-run \
  --package local/installers/falcon-sensor_<version>_amd64.deb
```

内容を確認したら、`--dry-run` を外して実行する。スクリプトはホスト、OS、アーキテクチャ、パッケージメタデータを検証してから sudo を要求する。

```bash
./scripts/install-falcon-sensor.sh \
  --package local/installers/falcon-sensor_<version>_amd64.deb
```

スクリプトは apt でパッケージを導入し、`falconctl` で CID を登録する。その後、`falcon-sensor.service` を有効化して起動する。CID はスクリプトの出力には表示しない。

## 端末登録まで確認して完了とする

ローカルでは、パッケージ、サービス、プロセスを確認する。

```bash
dpkg-query -W -f='${Status} ${Version}\n' falcon-sensor
systemctl is-enabled falcon-sensor.service
systemctl is-active falcon-sensor.service
sudo pgrep -a -x falcon-sensor
```

最後に、社内 IT 管理者へ Falcon Console の Newly Installed Sensors に `company-pop-new` が現れたことを確認してもらう。ローカルサービスが動いていても、Console に現れなければ導入完了とはしない。

## 更新にも同じスクリプトを使う

社内 IT 管理者から新しい `.deb` を受け取ったら、同じ手順で dry-run の後に再実行する。スクリプトは CID を再設定し、サービスが有効かつ稼働中であることを確認する。

アンインストール、CID の変更、proxy、sensor tag、provisioning token の追加は、このスクリプトでは扱わない。導入に失敗した場合も自動削除はせず、次の情報を添えて社内 IT 管理者へ連絡する。

```bash
sudo systemctl status falcon-sensor --no-pager
sudo journalctl -u falcon-sensor -n 50 --no-pager
dpkg-query -W falcon-sensor
```

## 用語と公式資料

- **Falcon Sensor**: 端末上で常駐する CrowdStrike のセンサー。本リポジトリでは `falcon-sensor.service` として扱う。
- **CID**: 端末を会社の Falcon テナントへ関連付ける Customer ID。ホストローカルの SOPS に保存する。
- **SOPS**: CID などを暗号化して保存し、対話シェルで環境変数として読み込む既存の秘密管理手段。

対応 OS と最低センサーバージョンは [CrowdStrike の展開 FAQ](https://www.crowdstrike.com/ja-jp/products/faq/)、基本的な Linux 導入手順は [Installing Falcon Sensor for Linux](https://www.crowdstrike.com/tech-hub/endpoint-security/installing-falcon-sensor-for-linux/) を参照する。この runbook で解決しない問題やポリシー判断は、作業を止めて社内 IT 管理者へ問い合わせる。

最終更新: 2026-07-21
