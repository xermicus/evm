use crate::error::{Error, TestError};
use crate::types::*;
use evm::backend::in_memory::{
	InMemoryAccount, InMemoryBackend, InMemoryEnvironment, InMemoryLayer,
};
use evm::standard::{Config, Etable, Gasometer, Invoker, TransactArgs};
use evm::utils::u256_to_h256;
use evm::{Capture, RuntimeState};
use primitive_types::U256;
use std::collections::{BTreeMap, BTreeSet};

pub fn run_test(_filename: &str, _test_name: &str, test: Test, debug: bool) -> Result<(), Error> {
	let config = match test.fork {
		Fork::Berlin => Config::berlin(),
		_ => return Err(Error::UnsupportedFork),
	};

	let env = InMemoryEnvironment {
		block_hashes: BTreeMap::new(), // TODO: fill in this field.
		block_number: test.env.current_number,
		block_coinbase: test.env.current_coinbase,
		block_timestamp: test.env.current_timestamp,
		block_difficulty: test.env.current_difficulty,
		block_randomness: Some(test.env.current_random),
		block_gas_limit: test.env.current_gas_limit,
		block_base_fee_per_gas: U256::zero(), // TODO: fill in this field.
		chain_id: U256::zero(),               // TODO: fill in this field.
	};

	let state = test
		.pre
		.clone()
		.into_iter()
		.map(|(address, account)| {
			let storage = account
				.storage
				.into_iter()
				.map(|(key, value)| (u256_to_h256(key), u256_to_h256(value)))
				.collect::<BTreeMap<_, _>>();

			(
				address,
				InMemoryAccount {
					balance: account.balance,
					code: account.code.0,
					nonce: account.nonce,
					original_storage: storage.clone(),
					storage,
				},
			)
		})
		.collect::<BTreeMap<_, _>>();

	let etable = Etable::runtime();
	let invoker = Invoker::new(&config);
	let args = TransactArgs::Call {
		caller: test.transaction.sender,
		address: test.transaction.to,
		value: test.transaction.value,
		data: test.transaction.data,
		gas_limit: test.transaction.gas_limit,
		gas_price: test.transaction.gas_price,
		access_list: Vec::new(),
	};

	let mut run_backend = InMemoryBackend {
		environment: env,
		layers: vec![InMemoryLayer {
			state,
			logs: Vec::new(),
			suicides: Vec::new(),
			hots: BTreeSet::new(),
		}],
	};
	let mut step_backend = run_backend.clone();

	// Run
	let run_result = evm::transact::<RuntimeState, Gasometer, _, _, _, _>(
		args.clone(),
		Some(4),
		&mut run_backend,
		&invoker,
		&etable,
	);

	// Step
	if debug {
		let _step_result = evm::HeapTransact::<RuntimeState, Gasometer, _, _, _>::new(
			args,
			&invoker,
			&mut step_backend,
		)
		.and_then(|mut stepper| loop {
			{
				let machine = stepper.last_machine()?;
				println!(
					"pc: {}, opcode: {:?}, gas: 0x{:x}",
					machine.machine.position(),
					machine.machine.peek_opcode(),
					machine.gasometer.gas(),
				);
			}
			if let Err(Capture::Exit(result)) = stepper.step(&etable) {
				break result;
			}
		});
	}

	let state_root = crate::hash::state_root(&run_backend);

	if test.post.expect_exception.is_some() {
		if run_result.is_err() {
			return Ok(());
		} else {
			return Err(TestError::ExpectException.into());
		}
	}

	if state_root != test.post.hash {
		if debug {
			for (address, account) in &run_backend.layers[0].state {
				println!(
					"address: {:?}, balance: {}, nonce: {}, code: 0x{}, storage: {:?}",
					address,
					account.balance,
					account.nonce,
					hex::encode(&account.code),
					account.storage
				);
			}
		}

		return Err(TestError::StateMismatch.into());
	}

	Ok(())
}
