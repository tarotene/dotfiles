//! hook を発火させるイベント。出力の形(`Reply::render`)がイベントごとに違う。

/// hook が受けるイベント。Stop は段3(Stop の結果を型にする)で足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    SessionStart,
    /// `ExitPlanMode` の許可要求。deny だけが `decision.behavior` の形で出る。
    PermissionRequest,
}

impl HookEvent {
    /// `hookSpecificOutput.hookEventName` に書く名前。
    pub fn name(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::PermissionRequest => "PermissionRequest",
        }
    }

    /// 標準入力の `hook_event_name` から決める。`PermissionRequest` 以外(空・未知を含む)は
    /// `PreToolUse`。plan gate が、PreToolUse と PermissionRequest の両方で同じ bin を使うための規則で、
    /// 従来の `jqfmt::deny_for_event` と同じ。
    pub fn from_input_name(name: &str) -> HookEvent {
        if name == "PermissionRequest" {
            HookEvent::PermissionRequest
        } else {
            HookEvent::PreToolUse
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_permission_request_is_special() {
        assert_eq!(
            HookEvent::from_input_name("PermissionRequest"),
            HookEvent::PermissionRequest
        );
        for other in ["PreToolUse", "PostToolUse", "SessionStart", "", "x"] {
            assert_eq!(HookEvent::from_input_name(other), HookEvent::PreToolUse);
        }
    }

    #[test]
    fn names_are_the_wire_names() {
        assert_eq!(HookEvent::PreToolUse.name(), "PreToolUse");
        assert_eq!(HookEvent::PostToolUse.name(), "PostToolUse");
        assert_eq!(HookEvent::SessionStart.name(), "SessionStart");
        assert_eq!(HookEvent::PermissionRequest.name(), "PermissionRequest");
    }
}
