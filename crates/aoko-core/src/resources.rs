//! Exclusive game-resource ownership.
//!
//! `packet-and-call-hygiene` (`.agents/skills/packet-and-call-hygiene/SKILL.md`
//! §2) names the resources that may have exactly **one owner per tick**: held
//! slot, use-item, attack/swing, window clicks and silent rotations. Two modules
//! driving one of them in the same tick is what triggers Grim/Vulcan/Polar
//! `BadPackets`, so the host has to model the conflict even though the bridge also
//! guards it.
//!
//! This module is the host half of that contract: a small, allocation-free
//! ownership table plus an RAII transaction handle for the AutoRod pattern
//! (`[own hotbar] -> use -> restore -> settle`), during which every other hotbar
//! owner must no-op.
//!
//! The C# host had no equivalent: `Clicker.cs` serialised by convention and the
//! bridges carried the transaction flags (`HasPendingAutoRodLegacyTransaction`,
//! `IsAutoRodTransactionActive121`). Modelling it here is what lets the ported
//! controllers be tested for conflicts instead of trusting call order.

use std::collections::BTreeMap;

use parking_lot::Mutex;

/// A resource that only one module may own at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resource {
    /// `C09` / `ServerboundSetCarriedItemPacket`: the selected hotbar slot.
    HeldSlot,
    /// `C08` / use-item / right-click.
    UseItem,
    /// `C0A` then `C02` / attack and swing.
    Attack,
    /// `C0E` / click-slot inside an open container.
    WindowClick,
    /// `C03` / silent rotation.
    Look,
}

impl Resource {
    /// Every resource, in the order the skill's conflict matrix lists them.
    pub const ALL: [Self; 5] = [
        Self::HeldSlot,
        Self::UseItem,
        Self::Attack,
        Self::WindowClick,
        Self::Look,
    ];

    /// Short stable name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HeldSlot => "held slot",
            Self::UseItem => "use item",
            Self::Attack => "attack",
            Self::WindowClick => "window click",
            Self::Look => "look",
        }
    }
}

/// Who holds a resource. Ordered by arbitration priority, not by importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    /// Clicker left/right clicks: the baseline owner of attack and use-item.
    Clicker,
    /// AutoRod: the exclusive hotbar transaction.
    AutoRod,
    /// AutoTool: hotbar swaps around a mining context.
    AutoTool,
    /// KillAura: attack plus optional auto-block use.
    KillAura,
    /// Triggerbot: attack.
    Triggerbot,
    /// Chest Stealer: window clicks.
    ChestStealer,
    /// Refill: window clicks.
    Refill,
}

impl Owner {
    /// Short stable name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clicker => "clicker",
            Self::AutoRod => "autorod",
            Self::AutoTool => "autotool",
            Self::KillAura => "killaura",
            Self::Triggerbot => "triggerbot",
            Self::ChestStealer => "cheststealer",
            Self::Refill => "refill",
        }
    }
}

/// Why an acquisition failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Another module already owns the resource this tick.
    Busy {
        /// The contested resource.
        resource: Resource,
        /// Who holds it.
        holder: Owner,
    },
}

/// The ownership table. Shared by every controller through an `Arc`.
#[derive(Debug, Default)]
pub struct ResourceLocks {
    holders: Mutex<BTreeMap<Resource, Owner>>,
}

impl ResourceLocks {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claims one resource.
    ///
    /// Re-acquiring a resource you already own succeeds, so a module can enter the
    /// same code path twice in a tick without deadlocking itself.
    ///
    /// # Errors
    /// [`Conflict::Busy`] when another owner holds it.
    pub fn try_acquire(&self, resource: Resource, owner: Owner) -> Result<(), Conflict> {
        let mut holders = self.holders.lock();
        match holders.get(&resource) {
            Some(holder) if *holder != owner => Err(Conflict::Busy {
                resource,
                holder: *holder,
            }),
            _ => {
                holders.insert(resource, owner);
                Ok(())
            }
        }
    }

    /// Claims every resource or none of them.
    ///
    /// # Errors
    /// [`Conflict::Busy`] naming the first contested resource; nothing is claimed.
    pub fn try_acquire_all(&self, resources: &[Resource], owner: Owner) -> Result<(), Conflict> {
        let mut holders = self.holders.lock();
        for resource in resources {
            if let Some(holder) = holders.get(resource) {
                if *holder != owner {
                    return Err(Conflict::Busy {
                        resource: *resource,
                        holder: *holder,
                    });
                }
            }
        }
        for resource in resources {
            holders.insert(*resource, owner);
        }
        Ok(())
    }

    /// Releases one resource. Only the owner may release, so a stale caller cannot
    /// free another module's claim.
    pub fn release(&self, resource: Resource, owner: Owner) -> bool {
        let mut holders = self.holders.lock();
        if holders.get(&resource) == Some(&owner) {
            holders.remove(&resource);
            true
        } else {
            false
        }
    }

    /// Releases every listed resource this owner holds.
    pub fn release_all(&self, resources: &[Resource], owner: Owner) {
        let mut holders = self.holders.lock();
        for resource in resources {
            if holders.get(resource) == Some(&owner) {
                holders.remove(resource);
            }
        }
    }

    /// Current holder of a resource.
    #[must_use]
    pub fn holder(&self, resource: Resource) -> Option<Owner> {
        self.holders.lock().get(&resource).copied()
    }

    /// Whether `owner` holds `resource`.
    #[must_use]
    pub fn is_held_by(&self, resource: Resource, owner: Owner) -> bool {
        self.holder(resource) == Some(owner)
    }

    /// Whether the resource is free.
    #[must_use]
    pub fn is_free(&self, resource: Resource) -> bool {
        !self.holders.lock().contains_key(&resource)
    }

    /// Drops every claim. Only for teardown and tests.
    pub fn clear(&self) {
        self.holders.lock().clear();
    }

    /// Begins the AutoRod exclusive transaction.
    ///
    /// # Errors
    /// [`Conflict::Busy`] when another module already owns the hotbar or the
    /// use-item path; the caller must no-op this tick.
    pub fn begin_hotbar_transaction(
        &self,
        owner: Owner,
    ) -> Result<ResourceTransaction<'_>, Conflict> {
        self.try_acquire_all(&[Resource::HeldSlot, Resource::UseItem], owner)?;
        Ok(ResourceTransaction {
            locks: self,
            owner,
            held: vec![Resource::HeldSlot, Resource::UseItem],
        })
    }
}

/// RAII claim over a set of resources.
///
/// Dropping it releases everything it owns, so a cancelled or panicking controller
/// cannot leak the hotbar.
#[derive(Debug)]
pub struct ResourceTransaction<'a> {
    locks: &'a ResourceLocks,
    owner: Owner,
    held: Vec<Resource>,
}

impl ResourceTransaction<'_> {
    /// Who owns the transaction.
    #[must_use]
    pub fn owner(&self) -> Owner {
        self.owner
    }

    /// Whether this transaction still owns `resource`.
    #[must_use]
    pub fn holds(&self, resource: Resource) -> bool {
        self.held.contains(&resource) && self.locks.is_held_by(resource, self.owner)
    }

    /// Ends the transaction early. Idempotent.
    pub fn finish(&mut self) {
        self.locks.release_all(&self.held, self.owner);
        self.held.clear();
    }
}

impl Drop for ResourceTransaction<'_> {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_owner_per_resource() {
        let locks = ResourceLocks::new();
        assert!(locks
            .try_acquire(Resource::HeldSlot, Owner::AutoRod)
            .is_ok());

        assert_eq!(
            locks.try_acquire(Resource::HeldSlot, Owner::AutoTool),
            Err(Conflict::Busy {
                resource: Resource::HeldSlot,
                holder: Owner::AutoRod
            })
        );
        assert_eq!(locks.holder(Resource::HeldSlot), Some(Owner::AutoRod));

        // The owner may re-enter its own claim.
        assert!(locks
            .try_acquire(Resource::HeldSlot, Owner::AutoRod)
            .is_ok());

        assert!(locks.release(Resource::HeldSlot, Owner::AutoRod));
        assert!(locks.is_free(Resource::HeldSlot));
    }

    #[test]
    fn only_the_owner_may_release() {
        let locks = ResourceLocks::new();
        assert!(locks
            .try_acquire(Resource::UseItem, Owner::KillAura)
            .is_ok());

        assert!(!locks.release(Resource::UseItem, Owner::AutoRod));
        assert_eq!(locks.holder(Resource::UseItem), Some(Owner::KillAura));
        assert!(locks.release(Resource::UseItem, Owner::KillAura));
    }

    #[test]
    fn acquire_all_is_atomic() {
        let locks = ResourceLocks::new();
        assert!(locks
            .try_acquire(Resource::UseItem, Owner::KillAura)
            .is_ok());

        assert_eq!(
            locks.try_acquire_all(&[Resource::HeldSlot, Resource::UseItem], Owner::AutoRod),
            Err(Conflict::Busy {
                resource: Resource::UseItem,
                holder: Owner::KillAura
            })
        );
        // The first resource of the failed set was not left claimed.
        assert!(locks.is_free(Resource::HeldSlot));
    }

    #[test]
    fn the_autorod_transaction_blocks_every_other_hotbar_owner() {
        let locks = ResourceLocks::new();

        let transaction = locks
            .begin_hotbar_transaction(Owner::AutoRod)
            .expect("a free table grants the transaction");
        assert!(transaction.holds(Resource::HeldSlot));
        assert!(transaction.holds(Resource::UseItem));

        assert!(locks
            .try_acquire(Resource::HeldSlot, Owner::AutoTool)
            .is_err());
        assert!(locks
            .try_acquire(Resource::UseItem, Owner::Clicker)
            .is_err());

        // Attack is a different resource and stays available.
        assert!(locks.try_acquire(Resource::Attack, Owner::KillAura).is_ok());

        drop(transaction);
        assert!(locks.is_free(Resource::HeldSlot));
        assert!(locks.is_free(Resource::UseItem));
    }

    #[test]
    fn finishing_a_transaction_early_is_idempotent() {
        let locks = ResourceLocks::new();
        let mut transaction = locks
            .begin_hotbar_transaction(Owner::AutoRod)
            .expect("free table");
        transaction.finish();
        assert!(locks.is_free(Resource::HeldSlot));
        transaction.finish();
        assert!(locks.is_free(Resource::HeldSlot));
    }

    #[test]
    fn a_contended_transaction_is_refused_without_claiming_anything() {
        let locks = ResourceLocks::new();
        assert!(locks
            .try_acquire(Resource::HeldSlot, Owner::AutoTool)
            .is_ok());

        let result = locks.begin_hotbar_transaction(Owner::AutoRod);
        assert!(result.is_err());
        assert!(locks.is_free(Resource::UseItem));
        assert_eq!(locks.holder(Resource::HeldSlot), Some(Owner::AutoTool));
    }

    #[test]
    fn every_resource_and_owner_has_a_name() {
        for resource in Resource::ALL {
            assert!(!resource.as_str().is_empty());
        }
        for owner in [
            Owner::Clicker,
            Owner::AutoRod,
            Owner::AutoTool,
            Owner::KillAura,
            Owner::Triggerbot,
            Owner::ChestStealer,
            Owner::Refill,
        ] {
            assert!(!owner.as_str().is_empty());
        }
    }

    #[test]
    fn clear_drops_every_claim() {
        let locks = ResourceLocks::new();
        assert!(locks
            .try_acquire_all(&Resource::ALL, Owner::Clicker)
            .is_ok());
        locks.clear();
        for resource in Resource::ALL {
            assert!(locks.is_free(resource));
        }
    }
}
