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
    use frame_support::BoundedVec;
    use frame_support::traits::ConstU32;

    // ProjectID used to uniquely identify a project
    pub type ProjectID = u32;
    pub type RoundID = u32;
    pub type VoterID = u32;

    #[derive(Encode, Decode, Default, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
    pub struct ProjectDetails<AccountId> {
        name: BoundedVec<u8, ConstU32<32>>,
        owner: AccountId,
        payment_destination: AccountId
    }

    #[derive(Encode, Decode, Default, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
    pub struct VoteDetails<AccountId> {
        account: AccountId,
        weight: u32
    }

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

        #[pallet::constant]
        type MaxProjects: Get<u32>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    #[pallet::type_value]
    pub fn DefaultID() -> u32 { 1 }

    #[pallet::storage]
    #[pallet::getter(fn next_round_id)]
    pub type NextRoundID<T: Config> = StorageValue<_, RoundID, ValueQuery, DefaultID>;

    #[pallet::storage]
    #[pallet::getter(fn next_project_id)]
    pub type NextProjectID<T: Config> = StorageValue<_, ProjectID, ValueQuery, DefaultID>;

    #[pallet::storage]
    #[pallet::getter(fn next_voter_id)]
    pub type NextVoterID<T: Config> = StorageValue<_, VoterID, ValueQuery, DefaultID>;

    // (coordinator_account_id) -> (round_id)
    // doing it this way so the coordinator can call functions without tracking the round_id,
    // only the users making a vote need to track the round_id
    // also prevents outside manipulation (i.e., non-coordinator ending voting, non-coordinator adding new project)
    #[pallet::storage]
    pub type Coordinator<T: Config> = StorageMap<_, Identity, <T as frame_system::Config>::AccountId, u32>;

    // (round_id) -> (is_active)
    // track if a specific round is active or not
    #[pallet::storage]
    pub type Round<T: Config> = StorageMap<_, Identity, RoundID, bool>;

    // (project_id) -> (project details)
    #[pallet::storage]
    pub type Project<T: Config> = StorageMap<_, Identity, ProjectID, ProjectDetails<T::AccountId>, OptionQuery>;

    // (project_owner_account_id) -> (project_id)
    #[pallet::storage]
    pub type ProjectOwner<T: Config> = StorageMap<_, Identity, <T as frame_system::Config>::AccountId, ProjectID>;

    // (round_id) -> [(project_id)]
    #[pallet::storage]
    pub type Projects<T: Config> = StorageMap<_, Blake2_128Concat, u32, BoundedVec<u32, T::MaxProjects>, ValueQuery>;

    // (account_id) -> (voter_id)
    #[pallet::storage]
    pub type Voter<T: Config> = StorageMap<_, Blake2_128Concat, <T as frame_system::Config>::AccountId, VoterID, ValueQuery>;

    // (round_id, project_id, account_id) -> (weight)
    // benefit of doing this is when a user votes more than once on a single project, only their most recent vote will be counted
    // and with an NMap we can iterate on a partial key (i.e., just the round and project IDs, so we can get each vote in batches by project_id)
    #[pallet::storage]
    pub type Vote<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, RoundID>,
            NMapKey<Blake2_128Concat, ProjectID>,
            NMapKey<Blake2_128Concat, VoterID>
        ),
        VoteDetails<T::AccountId>,
        OptionQuery
    >;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        NewVotingRound(RoundID),
        NewProjectCreated(ProjectID),
        ProjectRegistered(RoundID, ProjectID),
	}

	#[pallet::error]
	pub enum Error<T> {
        InvalidRoundID,
        OverflowRoundID,
        InvalidProjectID,
        ProjectAlreadyRegistered,
        ProjectNotRegistered,
        MaximumProjectsReached,
        NotCoordinator,
        CoordinatorAlreadyActive,
        RoundIsInactive,
        NotProjectOwner,
	}

	// Extrinsic Functions
	#[pallet::call]
	impl<T: Config> Pallet<T> {
        // !!! The case of a Coordinator running TWO rounds at once is currently not supported

        // COORDINATOR FUNCTION
        // The user who calls start_round will become the Coordinator
        // !!! the full ammount in the coordinator account will be the matching funds
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
        pub fn create_new_project(origin: OriginFor<T>, name: BoundedVec<u8, ConstU32<32>>, payment_destination: T::AccountId) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let project_id = Self::get_new_project_id();
            let project_details = ProjectDetails { owner: who, name, payment_destination };
            <Project<T>>::insert(project_id, project_details);

            Self::deposit_event(Event::NewProjectCreated(project_id));

            Ok(())
        }
        
        // PROJECT OWNER FUNCTION
        // associate a project with a voting round. Needs to be registered by the project owner.
        // cannot register the project if value for MaxProjects is reached
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
            // check if the project is already registerd
            if Self::is_project_registered(project_id, round_id) {
                Err(<Error<T>>::ProjectAlreadyRegistered)?
            }
            // all conditions passed, register the project
            <Projects<T>>::try_mutate(round_id, |projects_vec| {
                projects_vec.try_push(project_id)
            }).map_err(|_| <Error<T>>::MaximumProjectsReached)?;

            Self::deposit_event(Event::ProjectRegistered(project_id, round_id));

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

            // check if the project is registered to this round
            if !Self::is_project_registered(project_id, round_id) {
                Err(<Error<T>>::ProjectNotRegistered)?
            }

            // get VoterID for this user
            // user has a VoterID only so we can use it in the key for Vote
            let voter_id = Self::get_voter_id_for_user(&who);

            // apply vote
            let key = (round_id, project_id, voter_id);
            let details = VoteDetails { account: who, weight: weight };
            <Vote<T>>::insert(key, details);

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

    // Private Functions
    impl<T: Config> Pallet<T> {
        fn get_new_round_id() -> RoundID {
            let round_id = Self::next_round_id();
            let next_round_id: RoundID = round_id.wrapping_add(1);
            NextRoundID::<T>::put(next_round_id);
            round_id
        }

        fn get_new_project_id() -> ProjectID {
            let project_id = Self::next_project_id();
            let next_project_id: ProjectID = project_id.wrapping_add(1);
            NextProjectID::<T>::put(next_project_id);
            project_id
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

        fn is_project_registered(project_id: u32, round_id: u32) -> bool {
            match <Projects<T>>::try_get(round_id) {
                Ok(projects_vec) => {
                    if projects_vec.contains(&project_id) {
                        return true;
                    }
                },
                Err(_) => {}
            }
            return false;
        }

        fn get_voter_id_for_user(account_id: &<T as frame_system::Config>::AccountId) -> VoterID {
            match <Voter<T>>::try_get(account_id) {
                Ok(voter_id) => { return voter_id; },
                Err(_) => {
                    let voter_id = Self::next_voter_id();
                    let next_voter_id: VoterID = voter_id.wrapping_add(1);
                    <Voter<T>>::insert(account_id, next_voter_id);
                    return next_voter_id;
                }
            }
        }
 
        fn get_destination_account_for_project(project_id: u32) -> Option<T::AccountId> {
            match <Project<T>>::try_get(project_id) {
                Ok(details) => { Some(details.payment_destination) },
                Err(_) => { None }
            }
        }

        // distribute funds into project accounts
        fn perform_qf_and_distribute_funds(round_id: RoundID) {
            // get list of projects for this round_id
            let mut projects = vec![];
            match <Projects<T>>::try_get(round_id) {
                Ok(projects_vec) => {
                    projects = projects_vec.into_inner();
                },
                Err(_) => { }
            }
            // loop through each project
            for project_id in projects {
                // get account to distribute funds into
                let dest_acc = Self::get_destination_account_for_project(project_id);
                if dest_acc.is_none() { continue; }
                let destination = dest_acc.unwrap();
                // loop through each vote for this project
                let prefix_key = (round_id, project_id);
                for vote in <Vote<T>>::iter_prefix_values(prefix_key) {
                    // vote.account is the account_id for this voter
                    // vote.weight is the ammount the user has put up for this project
                }
            }
        }
    }
}
