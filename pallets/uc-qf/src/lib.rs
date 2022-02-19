#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

// #[cfg(test)]
// mod mock;

// #[cfg(test)]
// mod tests;

// #[cfg(feature = "runtime-benchmarks")]
// mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn something)]
	// Learn more about declaring storage items:
	// https://docs.substrate.io/v3/runtime/storage#declaring-storage-items
    pub type Vote<T> = StorageValue<_, u32>;
    // only just started looking into this
    // not sure how we can define multiple storage value types
    // unless we dont care?? and everything just goes under one general store
    // pub type WhitelistUser<T : frame_system::Config> = StorageValue<_, T::AccountId>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        // Announce when a user has signed up
        // I assume we don't actually want to do this??? Putting it here anyway
        // params[who]
        UserSignedUp(T::AccountId),
        // Save a vote
        // params [vote targed, who]
        SaveVote(u32, T::AccountId),
	}

	#[pallet::error]
	pub enum Error<T> {
        // Throw when a non-signed up user tries to vote
        NotSignedUp
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
        // Not entirely sure what we want in our parameters
        // I don't think we really want to whitelist (for simplicity sake)
        // so just get everything ready to accept votes in the active voting context
        #[pallet::weight(10_000)]
        pub fn begin_voting(origin: OriginFor<T>) -> DispatchResult {
            Ok(())
        }

        // I assume we would just write out the user who is signed up to the store?
        #[pallet::weight(10_000)]
        pub fn sign_up(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // I don't really know the semantics here
            // We want to write a user who signs up to a local store
            // That way we can check against the store when someone tries to vote
            // <WhitelistUser<T>>::put(who);
            Self::deposit_event(Event::UserSignedUp(who));

            Ok(())
        }

        // this will need to change
        // weight of a vote should be dynamic based on how many times that user has votes on a particular subject
        // during voting setup we might want to define what each first-level vote cost
        // something like: cost to the voter = (number of votes)^2
        #[pallet::weight(10_000)]
        pub fn vote(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // check if user is signed up
            // otherwise throw an InvalidVote error
            // Err(Error::<T>::NotSignedUp)?;

            // store a vote
            // Not sure what the "target" will look like so I'm just putting 3 for now
            <Vote<T>>::put(3);
            Self::deposit_event(Event::SaveVote(3, who));

            Ok(())
        }

        // I imagine we need some sort of "end" function.
        // probably not exposed out like this so that users can call it
        // Figured it would be more of an event-driven thing called internally
        #[pallet::weight(10_000)]
        pub fn end_voting(origin: OriginFor<T>) -> DispatchResult {
            Ok(())
        }

	}
}
