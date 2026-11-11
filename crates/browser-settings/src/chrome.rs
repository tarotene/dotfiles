//! Chrome の操作(`headless_chrome`)。専用プロファイルの Chrome を起動して接続し、終わったら閉じる。

use crate::Service;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// 専用プロファイル(`profile`)で Chrome を起動する。`headed` なら窓を出す(`login` 用)。
fn launch(profile: &Path, headed: bool) -> Result<Browser, String> {
    std::fs::create_dir_all(profile).map_err(|e| {
        format!(
            "プロファイルのディレクトリを作れない({}): {e}",
            profile.display()
        )
    })?;
    let options = LaunchOptions::default_builder()
        .headless(!headed)
        .user_data_dir(Some(profile.to_path_buf()))
        .idle_browser_timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Chrome の起動設定が不正: {e}"))?;
    Browser::new(options)
        .map_err(|e| format!("Chrome を起動できない(google-chrome が PATH に要る): {e}"))
}

/// 設定画面を開き、`steps` を実行して、項目が出るまで待つ。
fn open(tab: &Arc<Tab>, service: &Service) -> Result<(), String> {
    tab.navigate_to(&service.url)
        .and_then(|t| t.wait_until_navigated())
        .map_err(|e| format!("{} を開けない: {e}", service.url))?;
    for (i, step) in service.steps.iter().enumerate() {
        if let Some(sel) = &step.wait {
            tab.wait_for_element(sel)
                .map_err(|e| format!("steps[{i}] の wait `{sel}` が出ない: {e}"))?;
        }
        if let Some(sel) = &step.click {
            let el = tab
                .wait_for_element(sel)
                .map_err(|e| format!("steps[{i}] の click `{sel}` が出ない: {e}"))?;
            el.click()
                .map_err(|e| format!("steps[{i}] の click `{sel}` を押せない: {e}"))?;
        }
    }
    tab.wait_for_element(&service.field).map_err(|e| {
        format!(
            "項目 `{}` が出ない。ログインが切れているか、画面が変わった(login で入り直す): {e}",
            service.field
        )
    })?;
    Ok(())
}

fn evaluate_string(tab: &Arc<Tab>, js: &str) -> Result<Option<String>, String> {
    let v = tab
        .evaluate(js, false)
        .map_err(|e| format!("画面の評価に失敗: {e}"))?
        .value;
    Ok(v.and_then(|v| v.as_str().map(str::to_string)))
}

/// 項目の現在の値。項目が無ければ `None`。
pub fn read(profile: &Path, service: &Service) -> Result<Option<String>, String> {
    let browser = launch(profile, false)?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("タブを開けない: {e}"))?;
    open(&tab, service)?;
    evaluate_string(&tab, &crate::js_read(&service.field))
}

/// 項目に文を入れ、入った値を読み戻して返す。**保存の操作はしない**(人が行う)。
pub fn paste(profile: &Path, service: &Service, text: &str) -> Result<Option<String>, String> {
    let browser = launch(profile, false)?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("タブを開けない: {e}"))?;
    open(&tab, service)?;
    let ok = tab
        .evaluate(&crate::js_paste(&service.field, text), false)
        .map_err(|e| format!("貼り付けに失敗: {e}"))?
        .value
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !ok {
        return Err(format!("項目 `{}` が見つからない", service.field));
    }
    evaluate_string(&tab, &crate::js_read(&service.field))
}

/// 窓を出して設定画面を開き、人がログインを済ませるまで待つ(`wait_for_enter` が戻るまで)。
/// プロファイルに Cookie が残り、次から `read` などが入れる。
pub fn login(
    profile: &Path,
    service: &Service,
    wait_for_enter: impl FnOnce(),
) -> Result<(), String> {
    let browser = launch(profile, true)?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("タブを開けない: {e}"))?;
    tab.navigate_to(&service.url)
        .map_err(|e| format!("{} を開けない: {e}", service.url))?;
    wait_for_enter();
    Ok(())
}
