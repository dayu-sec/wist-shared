//! Shared filesystem path helpers.

pub const AGENTD_CONFIG_FILE: &str = "agentd.toml";
pub const LEGACY_AGENT_CONFIG_FILE: &str = "agent.toml";
pub const STATE_DIR: &str = "state";
pub const RUN_DIR: &str = "run";
pub const LOG_DIR: &str = "log";
pub const LOG_INPUTS_DIR: &str = "state/logs/file_inputs";
pub const ACTIONS_DIR: &str = "actions";
pub const AGENT_RUNTIME_FILE: &str = "agent_runtime.json";
pub const EXECUTION_QUEUE_FILE: &str = "execution_queue.json";
pub const WORKDIR_PLAN_FILE: &str = "plan.json";
pub const WORKDIR_RUNTIME_FILE: &str = "runtime.json";
pub const WORKDIR_STATE_FILE: &str = "state.json";
pub const WORKDIR_RESULT_FILE: &str = "result.json";
pub const REPORT_ENVELOPE_SUFFIX: &str = ".report.json";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_non_empty_and_internally_consistent() {
        for constant in [
            AGENTD_CONFIG_FILE,
            LEGACY_AGENT_CONFIG_FILE,
            STATE_DIR,
            RUN_DIR,
            LOG_DIR,
            LOG_INPUTS_DIR,
            ACTIONS_DIR,
            AGENT_RUNTIME_FILE,
            EXECUTION_QUEUE_FILE,
            WORKDIR_PLAN_FILE,
            WORKDIR_RUNTIME_FILE,
            WORKDIR_STATE_FILE,
            WORKDIR_RESULT_FILE,
            REPORT_ENVELOPE_SUFFIX,
        ] {
            assert!(!constant.is_empty(), "常量不应为空: {constant:?}");
        }
        // 新旧配置文件名必须不同，否则回退逻辑会自相矛盾。
        assert_ne!(AGENTD_CONFIG_FILE, LEGACY_AGENT_CONFIG_FILE);
        // 日志输入目录落在 state 目录下；报告信封以点开头（扩展名后缀）。
        assert!(LOG_INPUTS_DIR.starts_with(STATE_DIR));
        assert!(REPORT_ENVELOPE_SUFFIX.starts_with('.'));
    }
}
