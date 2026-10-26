use mangodisk_platform::{ControlledExecutable, SystemInventory};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone)]
pub(crate) struct SystemCapabilities {
    executable_names: HashSet<String>,
    executables: HashMap<String, ControlledExecutable>,
    developer_tools_complete: bool,
    filesystem_kinds: HashSet<String>,
    filesystem_complete: bool,
    capabilities: HashSet<String>,
    capabilities_complete: bool,
    os_version: String,
    inventory_complete: bool,
}
impl SystemCapabilities {
    pub(crate) fn tool_count(&self) -> usize {
        self.executable_names.len()
    }
    pub(crate) fn filesystem_count(&self) -> usize {
        self.filesystem_kinds.len()
    }
    pub(crate) fn capability_count(&self) -> usize {
        self.capabilities.len()
    }

    pub(crate) fn from_system(system: SystemInventory, inventory_complete: bool) -> Self {
        let mut executable_names = HashSet::new();
        let mut executables = HashMap::new();
        for tool in system.developer_tools {
            let name = normalize(&tool.name);
            executable_names.insert(name.clone());
            executables.insert(name, tool.executable);
        }
        Self {
            executable_names,
            executables,
            developer_tools_complete: system.developer_tools_complete,
            filesystem_kinds: system
                .filesystem_kinds
                .into_iter()
                .map(|value| normalize(&value))
                .collect(),
            filesystem_complete: system.filesystem_complete,
            capabilities: system
                .capabilities
                .into_iter()
                .map(|value| normalize(&value))
                .collect(),
            capabilities_complete: system.capabilities_complete,
            os_version: system.os_version,
            inventory_complete,
        }
    }

    /// Specialized operations must execute the absolute path captured by the
    /// inventory. Aliases are compile-time constants and never rule-provided.
    pub(crate) fn executable(&self, aliases: &[&str]) -> Option<ControlledExecutable> {
        aliases
            .iter()
            .find_map(|alias| self.executables.get(&normalize(alias)).cloned())
    }

    pub(crate) fn executable_inventory_complete(&self) -> bool {
        self.inventory_complete && self.developer_tools_complete
    }

    pub(crate) fn developer_tools_complete(&self) -> bool {
        self.developer_tools_complete
    }

    pub(crate) fn has_executable(&self, names: &[String]) -> bool {
        names
            .iter()
            .map(|value| normalize(value))
            .any(|value| self.executable_names.contains(&value))
    }

    pub(crate) fn os_version(&self) -> &str {
        &self.os_version
    }

    pub(crate) fn filesystem_complete(&self) -> bool {
        self.filesystem_complete
    }

    pub(crate) fn has_filesystem_kind(&self, values: &[String]) -> bool {
        values
            .iter()
            .map(|value| normalize(value))
            .any(|value| self.filesystem_kinds.contains(&value))
    }

    pub(crate) fn capabilities_complete(&self) -> bool {
        self.capabilities_complete
    }

    pub(crate) fn has_capability(&self, values: &[String]) -> bool {
        values
            .iter()
            .map(|value| normalize(value))
            .any(|value| self.capabilities.contains(&value))
    }
}
fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
