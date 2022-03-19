#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
pub use frame_support::sp_runtime::{traits::{AccountIdConversion, Saturating, Zero, Hash}};

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

    #[derive(Encode, Decode, Default, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
    pub struct ProjectDetails<AccountId> {
        owner: AccountId,
        destination: AccountId
    }

    type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
    type ProjectDetailsOf<T> = ProjectDetails<AccountIdOf<T>>;

    // ID generation storage items
    // keeping track of the next id to distribute
    #[pallet::type_value]
    pub fn DefaultID<T: Config>() -> u32 { 1 }
    #[pallet::storage]
    pub type RoundID<T> = StorageValue<_, u32, ValueQuery, DefaultID<T>>;
    #[pallet::storage]
    pub type ProjectID<T> = StorageValue<_, u32, ValueQuery, DefaultID<T>>;

    // (coordinator_account_id) -> (round_id)
    // doing it this way so the coordinator can call functions without tracking the round_id,
    // only the users making a vote need to track the round_id
    // also prevents outside manipulation (i.e., non-coordinator ending voting, non-coordinator adding new project)
    #[pallet::storage]
    pub type Coordinator<T> = StorageMap<_, Identity, AccountIdOf<T>, u32>;

    // (round_id) -> (is_active)
    // track if a specific round is active or not
    #[pallet::storage]
    pub type Round<T> = StorageMap<_, Identity, u32, bool>;

    // (project_id) -> (project details)
    #[pallet::storage]
    pub type Project<T: Config> = StorageMap<_, Blake2_128Concat, u32, ProjectDetailsOf<T>, OptionQuery>;

    // (project_owner_account_id) -> (project_id)
    #[pallet::storage]
    pub type ProjectOwner<T> = StorageMap<_, Identity, AccountIdOf<T>, u32>;

    // #[pallet::type_value]
    // pub fun DefaultList<T: Config>() -> vec! { Vec::new() }

    // (round_id) -> [(project_id)]
    // #[pallet::storage]
    // pub type Projects<T> = StorageMap<_, Identity, u32, vec!, ValueQuery, DefaultList<T>>;

    // (round_id, project_id, account_id) -> (weight)
    // benefit of doing this is when a user votes more than once on a single project, only their most recent vote will be counted
    // and with an NMap we can iterate on a partial key (i.e., just the round and project IDs, so we can get each vote in batches by project_id)
    #[pallet::storage]
    pub type Vote<T> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, u32>,
            NMapKey<Blake2_128Concat, u32>,
            NMapKey<Blake2_128Concat, AccountIdOf<T>>
        ),
        u32,
        ValueQuery
    >;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        NewVotingRound(u32),
        NewProjectCreated(u32),
	}

	#[pallet::error]
	pub enum Error<T> {
        InvalidRoundID,
        InvalidProjectID,
        NotCoordinator,
        CoordinatorAlreadyActive,
        RoundIsInactive,
        NotProjectOwner,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
        // !!! The case of a Coordinator running TWO rounds at once is currently not supported
        // Personally I believe we should deny the ability for one Coordinator to run two rounds
        // and that decision is for simplicity sake, we still need to add a check if a round is active for Coordinator and throw an error if they try to start a new one

        // COORDINATOR FUNCTION
        // The user who calls start_round will become the Coordinator
        // lets go with this: the full ammount in the coordinator account will be the matching funds
        // its easy enough to create a new wallet and move money into it, so this is a fine solution
        #[pallet::weight(100)]
        pub fn start_round(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;

            if Self::is_coordinator_active(&who) {
                Err(Error::<T>::CoordinatorAlreadyActive)?
            }

            let round_id = Self::get_new_round_id();
            // assign round_id to the coordinator
            <Coordinator<T>>::insert(&who, round_id);
            // set the round as active
            <Round<T>>::insert(round_id, true);
            Self::deposit_event(Event::NewVotingRound(round_id));

            Ok(())
        }

        // PROJECT OWNER FUNCTION
        // the user who calls create_new_project will become the Project Owner
        #[pallet::weight(100)]
        pub fn create_new_project(origin: OriginFor<T>, destination_account: T::AccountId) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let project_id = Self::get_new_project_id();
            let project_details = ProjectDetails { owner: who, destination: destination_account };
            <Project<T>>::insert(project_id, project_details);

            Self::deposit_event(Event::NewProjectCreated(project_id));

            Ok(())
        }
        
        // PROJECT OWNER FUNCTION
        // associate a project with a voting round. Needs to be registered by the project owner.
        #[pallet::weight(100)]
        pub fn register_project(origin: OriginFor<T>, round_id: u32, project_id: u32) -> DispatchResult {
            // check if project exists
            let project = <Project<T>>::try_get(project_id);
            if project.is_err() {
                Err(Error::<T>::InvalidProjectID)?
            }
            // check if round exists and is active
            match <Round<T>>::try_get(round_id) {
                Ok(is_active) => {
                    if !is_active {
                        Err(Error::<T>::RoundIsInactive)?
                    }
                },
                Err(_) => Err(Error::<T>::InvalidRoundID)?
            }
            // check if current user is the project owner
            let details = project.unwrap();
            let who = ensure_signed(origin)?;
            if details.owner != who {
                Err(Error::<T>::NotProjectOwner)?
            }
            // all conditions passed, register the proejct

            // TODO
            // create a new record for this project with this round
            // saving this for later because I don't really konw how to do it yet
            // something like <Projects<T>> which is a StorageMap of (round_id) -> (project_ids: Vec<u32>) 
            // <Projects<T>>::try_push(round_id, project_id);

            Ok(())
        }
        
        // VOTER FUNCTION
        // records a vote
        #[pallet::weight(100)]
        pub fn vote(origin: OriginFor<T>, round_id: u32, project_id: u32, weight: u32) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // check if the round is active
            if !<Round<T>>::contains_key(round_id) { // round has never been registered
                Err(Error::<T>::InvalidRoundID)?
            } else if <Round<T>>::get(round_id).unwrap_or(false) == false { // round is set as inactive
                Err(Error::<T>::InvalidRoundID)?
            }

            // TODO
            // get list of projects for this round

            // TODO
            // make sure this project_id is in the list

            // apply vote
            // will need a new <Vote<T>> StorageMap
            //round_id, project_id, account_id) -> (weight)
            let key = (round_id, project_id, who);
            match <Vote<T>>::try_get(&key) {
                Ok(_) => {
                    <Vote<T>>::mutate(&key, |old_weight| {
                        *old_weight = weight;
                    });
                },
                Err(_) => {
                    <Vote<T>>::insert(&key, weight);
                }
            }

            Ok(())
        }

        // COORDINATOR FUNCTION
        // invalidates round and coordinator records, distributes funds
        #[pallet::weight(100)]
        pub fn end_round(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // end_round may only be called by the Coordinator, if anyone besides the Coordinator tries to execute this, throw an error
            let result = <Coordinator<T>>::try_get(&who);
            if result.is_err() {
                Err(Error::<T>::NotCoordinator)?
            }
            // retrieve the round this Coordinator is related to
            let round_id = result.unwrap_or(0);
            if round_id == 0 {
                Err(Error::<T>::InvalidRoundID)?
            }
            // TODO: collect all stored votes, perform QF
            Self::perform_qf_and_distribute_funds(round_id);
            // set coordinators round to nothing
            <Coordinator<T>>::remove(&who);
            // set the round as inactive
            <Round<T>>::remove(round_id);
            
            Ok(())
        }

	}

    // private functions (helpers)
    impl<T: Config> Pallet<T> {
        fn get_new_round_id() -> u32 {
            let round_id = RoundID::<T>::get();
            RoundID::<T>::put(round_id.wrapping_add(1));
            return round_id;
        }

        fn get_new_project_id() -> u32 {
            let project_id = ProjectID::<T>::get();
            ProjectID::<T>::put(project_id.wrapping_add(1));
            return project_id;
        }

        // Check if a given coordinator is associated with an active round
        fn is_coordinator_active(coordinator: &<T as frame_system::Config>::AccountId) -> bool {
            // Try and find the Coordinator ID in storage
            let coord = <Coordinator<T>>::try_get(coordinator);
            let round_id;
            match coord {
                Ok(round) => round_id = round,
                Err(_) => return false, // no coordinator found, not active
            }
            // Try and get the round the coordinator is associated with
            let round = <Round<T>>::try_get(round_id);
            match round {
                Ok(is_active) => return is_active,
                Err(_) => return false // no round found, not active
            }
        }

        // distribute funds into project accounts
        fn perform_qf_and_distribute_funds(round_id: u32) {

        }
    }
}
