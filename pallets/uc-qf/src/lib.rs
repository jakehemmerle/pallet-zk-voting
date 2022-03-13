#![cfg_attr(not(feature = "std"), no_std)]

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
    use frame_support::inherent::Vec;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    #[derive(Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, Default, TypeInfo)]
    pub struct ProjectDetails<AccountId> {
        name: Vec<u8>,
        owner: AccountId,
        destination: AccountId
    }

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
    pub type Coordinator<T> = StorageMap<_, Identity, <T as frame_system::Config>::AccountId, u32>;

    // (round_id) -> (is_active)
    // track if a specific round is active or not
    #[pallet::storage]
    pub type Round<T> = StorageMap<_, Identity, u32, bool>;

    // (project_id) -> (project details)
    #[pallet::storage]
    pub type Project<T: Config> = StorageMap<_, Identity, u32, ProjectDetails<T::AccountId>, ValueQuery>;

    // (project_owner_account_id) -> (project_id)
    #[pallet::storage]
    pub type ProjectOwner<T> = StorageMap<_, Identity, <T as frame_system::Config>::AccountId, u32>;

    // votes storage type should have the key (round_id, project_id, account_id) and value (weight)
    // this might be better as (round_id, project_id) -> (Vec<(account_id, weight)>)
    // #[pallet::storage]
    // pub type Vote<T>

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
        // create_new_project(name: &str, address: T::AccountId) -> project_id: u32
        // - make a new project by providing a name and an address. Returns a project ID
        #[pallet::weight(100)]
        // pub fn create_new_project<'a>(origin: OriginFor<T>, name: &'a str, address: T::AccountId) -> DispatchResult {
        pub fn create_new_project(origin: OriginFor<T>, name: Vec<u8>, address: T::AccountId) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // let name_string = String::from_utf8_lossy(&name);
            // @Question: is address the account where funds will be distributed?
            // or is address supposed to be the project owner?

            // I like the idea of address being the funds destination
            // with the user who calls create_new_project being the project owner

            let project_id = Self::get_new_project_id();
            let project_details = ProjectDetails { name: name, owner: who, destination: address };
            <Project<T>>::insert(project_id, project_details);

            Self::deposit_event(Event::NewProjectCreated(project_id));

            Ok(())
        }
        
        // PROJECT OWNER FUNCTION
        // register_project(round_id: u32, project_id) -> NotProjectOwner | ProjectRegistered
        // - associate a project with a voting round. Needs to be registered by the project owner.
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
            // something like <Projects<T>> which is a StorageMap of (round_id) -> (project_ids: Vec<u32>) 

            Ok(())
        }
        
        // VOTER FUNCTION
        // vote(round_id: u32, project_id, weight: u8) -> 
        // @Question: is the weight supposed to be how much the user is committing? Shouldn't that be much larger than u8?
        #[pallet::weight(100)]
        pub fn vote(origin: OriginFor<T>, round_id: u32, project_id: u32, weight: u8) -> DispatchResult {
            // check if the round is active
            if !<Round<T>>::contains_key(round_id) { // round has never been registered
                Err(Error::<T>::InvalidRoundID)?
            } else if <Round<T>>::get(round_id).unwrap_or(false) == false { // round is set as inactive
                Err(Error::<T>::InvalidRoundID)?
            }

            // apply vote
            // will need a new <Vote<T>> StorageMap

            // store as (round_id, project_id, voter_id) -> weight
            // benefit of doing this is when a user votes more than once on a single project, only their most recent vote will be counted
            // and with an NMap we can iterate on a partial key (i.e., just the round and project IDs, so we can get each vote in batches by project_id)

            Ok(())
        }

        // COORDINATOR FUNCTION
        // end_round (round_id: u32)
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

        fn perform_qf_and_distribute_funds(round_id: u32) {

        }
    }
}
