// Copyright 2018 Commonwealth Labs, Inc.
// This file is part of Edgeware.

// Edgeware is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Edgeware is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Edgeware.  If not, see <http://www.gnu.org/licenses/>.

#![feature(advanced_slice_patterns, slice_patterns)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate serde;

// Needed for deriving `Serialize` and `Deserialize` for various types.
// We only implement the serde traits for std builds - they're unneeded
// in the wasm runtime.
#[cfg(feature = "std")]

extern crate parity_codec as codec;
extern crate substrate_primitives as primitives;
#[cfg_attr(not(feature = "std"), macro_use)]
extern crate sr_std as rstd;
extern crate srml_support as runtime_support;
extern crate sr_primitives as runtime_primitives;
extern crate sr_io as runtime_io;

extern crate srml_balances as balances;
extern crate srml_system as system;
extern crate srml_democracy as democracy;

use democracy::{Approved, VoteThreshold};

use primitives::ed25519::Signature;
use runtime_primitives::traits::{Zero, As};

use runtime_primitives::traits::{MaybeSerializeDebug};
use rstd::prelude::*;
use system::ensure_signed;
use runtime_support::{StorageValue, StorageMap, Parameter};
use runtime_support::dispatch::Result;
use primitives::ed25519;

/// Record indices.
pub type DepositIndex = u32;
pub type WithdrawIndex = u32;

pub trait Trait: balances::Trait {
    /// The overarching event type.
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
}

pub type LinkedProof = Vec<u8>;

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        fn deposit_event() = default;

        /// The deposit function should always succeed (in order) a deposit transaction
        /// on the eligible blockchain that has an established two-way peg with Edgeware.
        /// This function can be triggered by the depositor or any bridge authority that
        /// sees the transaction first.
        pub fn deposit(origin, target: T::AccountId, tx_hash: T::Hash, quantity: T::Balance) -> Result {
            let _sender = ensure_signed(origin)?;
            
            // Match on deposit records by the respective transaction hash on the eligible blockchain
            match <DepositOf<T>>::get(tx_hash) {
                Some((inx, tgt, qty, signers)) => {
                    // Ensure all parameters match for safety
                    ensure!(tgt == target.clone(), "Accounts do not match");
                    ensure!(qty == quantity, "Quantities don't match");
                    // Ensure sender is a bridge authority if record exists
                    ensure!(Self::authorities().iter().any(|id| id == &_sender), "Invalid non-authority sender");
                    // Ensure senders can't sign twice
                    ensure!(!signers.iter().any(|id| id == &_sender), "Invalid duplicate deposit signings");
                    // Add record update with new signer
                    let mut new_signers = signers;
                    new_signers.push(_sender);
                    <DepositOf<T>>::insert(tx_hash, (inx, tgt.clone(), qty, new_signers.clone()));

                    // Check if we have reached enough signers for the deposit
                    let stake_sum = new_signers.iter()
                        .map(|s| <AuthorityStake<T>>::get(s))
                        .fold(Zero::zero(), |a,b| a + b);

                    // Check if we approve the proposal
                    let total_issuance = <balances::Module<T>>::total_issuance();
                    if VoteThreshold::SuperMajorityApprove.approved(stake_sum, Zero::zero(), total_issuance) {
                        <balances::Module<T>>::increase_free_balance_creating(&tgt, qty);
                    }
                },
                None => {
                    let index = Self::deposit_count();
                    <DepositCount<T>>::mutate(|i| *i += 1);
                    let mut signers = vec![];
                    if <Authorities<T>>::get().iter().any(|a| a == &_sender) {
                        signers.push(_sender);
                    }

                    <DepositOf<T>>::insert(tx_hash, (index, target, quantity, signers))
                },
            }

            Ok(())
        }

        /// The withdraw function should precede (in order) a withdraw transaction on the
        /// eligible blockchain that has an established two-way peg with Edgeware. This
        /// function should only be called by a token holder interested in transferring
        /// native Edgeware tokens with Edgeware-compliant, non-native tokens like ERC20.
        pub fn withdraw(origin, target: T::AccountId, quantity: T::Balance) -> Result {
            unimplemented!()
        }
    }
}

/// An event in this module.
decl_event!(
    pub enum Event<T> where <T as system::Trait>::Hash,
                            <T as system::Trait>::AccountId,
                            <T as balances::Trait>::Balance {
        // Deposit event for an account, an eligible blockchain transaction hash, and quantity
        Deposit(AccountId, Hash, Balance),
        // Withdraw event for an account, and an amount
        Withdraw(AccountId, Balance),
    }
);

decl_storage! {
    trait Store for Module<T: Trait> as IdentityStorage {
        /// Mapping from an eligible blockchain by Hash(name) to the list of block headers
        /// TODO: V2 feature when we have stronger proofs of transfers
        pub BlockHeaders get(block_headers): map T::Hash => Vec<T::Hash>;

        /// The active set of bridge authorities who can sign off on requests
        pub Authorities get(authorities): Vec<T::AccountId>;
        /// Mappings of stake per active authority
        pub AuthorityStake get(authority_stake): map T::AccountId => T::Balance;
        /// Total stake managed by the bridge authorities
        pub TotalAuthorityStake get(total_authority_stake): T::Balance;
        /// The required stake threshold for executing requests represented as an integer [0,100]
        pub StakeThreshold get(stake_threshold) config(): T::Balance;

        /// Number of deposits
        pub DepositCount get(deposit_count): u32;
        /// List of all deposit requests on Edgeware taken to be the transaction hash
        /// from the eligible blockchain
        pub Deposits get(deposits): Vec<T::Hash>;
        /// Mapping of deposit transaction hashes from the eligible blockchain to the
        /// deposit request record
        pub DepositOf get(deposit_of): map T::Hash => Option<(DepositIndex, T::AccountId, T::Balance, Vec<T::AccountId>)>;
        
        /// Number of withdraws
        pub WithdrawCount get(withdraw_count): u32;
        /// List of all withdraw requests on Edgeware taken to be the unique hash created
        /// on Edgeware with the user's account, quantity, and nonce
        pub Withdraws get(withdraws): Vec<T::Hash>;
        /// Mapping of withdraw record hashes to the record
        pub WithdrawOf get(withdraw_of): map T::Hash => Option<(WithdrawIndex, T::AccountId, T::Balance, Vec<T::AccountId>)>;
        /// Nonce for creating unique hashes per user per withdraw request
        pub WithdrawNonceOf get(withdraw_nonce_of): map T::AccountId => u32;
    }
}
