use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::Principal;
use crate::{Result, name, require};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub read_roles: BTreeSet<String>,
    pub write_roles: BTreeSet<String>,
}

impl Permission {
    fn allows(&self, actor: &Principal, write: bool) -> bool {
        let roles = if write {
            &self.write_roles
        } else {
            &self.read_roles
        };
        !roles.is_disjoint(&actor.roles)
    }
}

/// An application ACL, not the proposed native actor-view implementation.
/// A missing type, entity, or property permission denies access. All three
/// levels must allow an operation. No client-provided ACL is authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    pub type_permission: Permission,
    pub entities: BTreeMap<String, Permission>,
    pub properties: BTreeMap<String, Permission>,
}

impl Access {
    pub fn validate(&self) -> Result<()> {
        require(
            self.entities.len() <= 8192 && self.properties.len() <= 256,
            "access contract exceeds entity or property budget",
        )?;
        for key in self.entities.keys().chain(self.properties.keys()) {
            name(key)?;
        }
        for permission in std::iter::once(&self.type_permission)
            .chain(self.entities.values())
            .chain(self.properties.values())
        {
            require(
                permission.read_roles.len() <= 64 && permission.write_roles.len() <= 64,
                "at most 64 roles per permission",
            )?;
            for role in permission.read_roles.iter().chain(&permission.write_roles) {
                name(role)?;
            }
        }
        Ok(())
    }

    pub fn allows(&self, actor: &Principal, entity: &str, property: &str, write: bool) -> bool {
        actor.validate().is_ok()
            && self.type_permission.allows(actor, write)
            && self
                .entities
                .get(entity)
                .is_some_and(|p| p.allows(actor, write))
            && self
                .properties
                .get(property)
                .is_some_and(|p| p.allows(actor, write))
    }

    /// Apply before any aggregation, ranking, traversal, or serialization.
    /// The caller must not reuse unrestricted search scores with this output.
    pub fn visible(
        &self,
        actor: &Principal,
        entity: &str,
        properties: &BTreeMap<String, Value>,
    ) -> Result<Option<BTreeMap<String, Value>>> {
        self.validate()?;
        actor.validate()?;
        if !self.type_permission.allows(actor, false)
            || !self
                .entities
                .get(entity)
                .is_some_and(|p| p.allows(actor, false))
        {
            return Ok(None);
        }
        Ok(Some(
            properties
                .iter()
                .filter(|(key, _)| self.allows(actor, entity, key, false))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ))
    }

    /// A partial update preserves unmentioned hidden properties. No failure
    /// includes a hidden value or property name in its error payload.
    pub fn validate_update(
        &self,
        actor: &Principal,
        entity: &str,
        changed_properties: &BTreeSet<String>,
    ) -> Result<()> {
        self.validate()?;
        actor.validate()?;
        require(!changed_properties.is_empty(), "empty update")?;
        require(
            changed_properties
                .iter()
                .all(|key| self.allows(actor, entity, key, true)),
            "write outside the actor view",
        )
    }
}
