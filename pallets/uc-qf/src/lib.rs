#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
pub use frame_support::sp_runtime::{traits::{AccountIdConversion, Saturating, Zero, Hash}};

#[frame_support::pallet]
pub mod pallet {
	use frame_system::pallet_prelude::*;
    use frame_support::{
        pallet_prelude::*,
        BoundedVec,
        inherent::Vec,
        traits::{ConstU32, Currency, ExistenceRequirement}
    };
    #[allow(unused_imports)]
    use integer_sqrt::IntegerSquareRoot;

    // ProjectID used to uniquely identify a project
    pub type ProjectID = u32;
    pub type RoundID = u32;

    #[derive(Encode, Decode, Default, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
    pub struct ProjectDetails<AccountId> {
        name: BoundedVec<u8, ConstU32<32>>,
        owner: AccountId,
        payment_destination: AccountId,
        voting_power: u128
    }

    #[derive(Encode, Decode, Default, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
    pub struct VoteDetails<AccountId, Balance> {
        account: AccountId,
        balance: Balance
    }

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        type Currency: Currency<Self::AccountId>;
        #[pallet::constant]
        type MaxProjects: Get<u32>;
        #[pallet::constant]
        type MaxVotes: Get<u32>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::type_value]
    pub fn DefaultID() -> u32 { 1 }

    #[pallet::storage]
    #[pallet::getter(fn next_round_id)]
    pub type NextRoundID<T: Config> = StorageValue<_, RoundID, ValueQuery, DefaultID>;

    #[pallet::storage]
    #[pallet::getter(fn next_project_id)]
    pub type NextProjectID<T: Config> = StorageValue<_, ProjectID, ValueQuery, DefaultID>;

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

    // (round_id, project_id) -> [VoteDetails]
    // does not protect against a user submitting multiple votes for the same project
    #[pallet::storage]
    pub type Vote<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, RoundID>,
            NMapKey<Blake2_128Concat, ProjectID>
        ),
        BoundedVec<VoteDetails<T::AccountId, BalanceOf<T>>, T::MaxVotes>,
        ValueQuery
    >;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        NewVotingRound(RoundID),
        NewProjectCreated(ProjectID),
        ProjectRegistered(RoundID, ProjectID),
        LogErrNoProjectsForRound(),
        LogConversionFailed()
	}

	#[pallet::error]
	pub enum Error<T> {
        InvalidRoundID,
        OverflowRoundID,
        InvalidProjectID,
        ProjectAlreadyRegistered,
        ProjectNotRegistered,
        MaximumProjectsReached,
        MaximumVotesReached,
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
            let project_details = ProjectDetails { owner: who, name, payment_destination, voting_power: 0_u128 };
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
        pub fn vote(origin: OriginFor<T>, round_id: u32, project_id: u32, balance: BalanceOf<T>) -> DispatchResult {
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

            // apply vote
            let details = VoteDetails { account: who, balance };
            <Vote<T>>::try_mutate(Self::generate_vote_key(round_id, project_id), |votes_vec| {
                votes_vec.try_push(details)
            }).map_err(|_| <Error<T>>::MaximumVotesReached)?;

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
            // !!! Invalidate the round first so no new votes can be entered
            // set coordinators round to nothing
            <Coordinator<T>>::remove(&who);
            // set the round as inactive
            <Round<T>>::remove(round_id);

            // collect all stored votes, perform QF
            Self::perform_qf_and_distribute_funds(round_id, &who);

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

        fn generate_vote_key(round_id: RoundID, project_id: ProjectID) -> (RoundID, ProjectID) {
            (round_id, project_id)
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
 
        fn get_destination_account_for_project(project_id: u32) -> Option<T::AccountId> {
            match <Project<T>>::try_get(project_id) {
                Ok(details) => { Some(details.payment_destination) },
                Err(_) => { None }
            }
        }

        fn get_votes_for_project(project_id: ProjectID, round_id: RoundID) -> Vec<VoteDetails<T::AccountId, BalanceOf<T>>> {
            let mut votes: Vec<VoteDetails<T::AccountId, BalanceOf<T>>> = Vec::new();
            match <Vote<T>>::try_get(Self::generate_vote_key(round_id, project_id)) {
                Ok(votes_vec) => { votes = votes_vec.into_inner() },
                Err(_) => {}
            }
            votes
        }

        // voting power is SQUARE(SUM(SQRT(weight_i)))
        fn get_voting_power_for_project(project_id: u32, round_id: u32) -> u128 {
            match <Project<T>>::try_get(project_id)  {
                Ok(mut details) => {
                    if details.voting_power == 0_u128 {
                        let mut voting_power = 0_u128;
                        for vote in Self::get_votes_for_project(project_id, round_id) {
                            let balance128 = Self::balance_as_128(vote.balance);
                            voting_power += balance128.integer_sqrt();
                        }
                        voting_power = u128::pow(voting_power, 2);
                        // update voting_power on the project record
                        details.voting_power = voting_power;
                        <Project<T>>::insert(project_id, details);
                        return voting_power;
                    } else {
                        details.voting_power
                    }
                },
                Err(_) => { 0_u128 }
            }
        }

        // distribute funds into project accounts
        fn perform_qf_and_distribute_funds(round_id: RoundID, matching_funds_account: &T::AccountId) {
            // get list of projects for this round_id
            let projects: Vec<u32>;
            match <Projects<T>>::try_get(round_id) {
                Ok(projects_vec) => { projects = projects_vec.into_inner() },
                Err(_) => {
                    Self::deposit_event(Event::LogErrNoProjectsForRound());
                    return;
                }
            }
            // Get the amount of funds we have available
            let matching_balance = Self::get_wallet_balance(matching_funds_account);
            let matching_funds = Self::balance_as_128(matching_balance);
            // First loop: calculate voting power for each project
            let mut total_voting_power = 0_u128;
            for project_id in &projects {
                total_voting_power += Self::get_voting_power_for_project(*project_id, round_id);
            }
            // Second loop: distribute funds
            for project_id in &projects {
                // get account to distribute funds into
                let destination: T::AccountId;
                match Self::get_destination_account_for_project(*project_id) {
                    Some(account) => { destination = account },
                    None => { continue; }
                }
                // distribute funds each voter has promised
                for vote in Self::get_votes_for_project(*project_id, round_id) {
                    Self::distribute_funds(&destination, &vote.account, vote.balance);
                }
                // distribute matching funds
                if total_voting_power == 0 { return; }
                let voting_power = Self::get_voting_power_for_project(*project_id, round_id);
                let distribution_ratio = (voting_power as f64)/(total_voting_power as f64);

                // ammount to be distributed is distribution_ratio * matching_funds
                let mut project_matched_funds = ((matching_funds as f64)*distribution_ratio) as u128;
                // subtract 100k so gas can be paid
                project_matched_funds -= 100000;
                match Self::u128_to_balance(project_matched_funds) {
                    Some(balance) => {
                        Self::distribute_funds(&destination, matching_funds_account, balance);
                    },
                    None => {
                        Self::deposit_event(Event::LogConversionFailed());
                    }
                }
            }
        }

        fn balance_as_128(balance: BalanceOf<T>) -> u128 {
            let b128_option: Option<u128> = balance.try_into().ok();
            match b128_option {
                Some(b128) => { b128 },
                None => { 0 }
            }
        }

        fn u128_to_balance(value: u128) -> Option<BalanceOf<T>> {
            value.try_into().ok()
        }

        fn get_wallet_balance(account: &T::AccountId) -> BalanceOf<T> {
            T::Currency::free_balance(account)
        }

        fn distribute_funds(destination_account: &T::AccountId, source_account: &T::AccountId, balance: BalanceOf<T>) {
            let _ = T::Currency::transfer(source_account, destination_account, balance, ExistenceRequirement::KeepAlive);
        }

    }
}
