//! Shared ID helpers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceId(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_compare_by_value() {
        assert_eq!(AgentId("a".into()), AgentId("a".into()));
        assert_ne!(AgentId("a".into()), AgentId("b".into()));
        assert_ne!(InstanceId("a".into()), InstanceId("b".into()));
    }

    #[test]
    fn ids_debug_is_readable() {
        assert_eq!(
            format!("{:?}", AgentId("agent-1".into())),
            "AgentId(\"agent-1\")"
        );
    }
}
