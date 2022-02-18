#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    instruction::InstructionError,
    pubkey::{Pubkey, PUBKEY_BYTES},
    signature::{Keypair, Signer},
    system_instruction::create_account,
    transaction::{Transaction, TransactionError},
};
use spl_token::{instruction::approve, solana_program::program_pack::Pack};
use spl_token_lending::{
    error::LendingError,
    instruction::flash_borrow_reserve_liquidity,
    instruction::{
        borrow_obligation_liquidity, deposit_obligation_collateral, flash_repay_reserve_liquidity,
        init_obligation, refresh_obligation, refresh_reserve, repay_obligation_liquidity,
        withdraw_obligation_collateral,
    },
    math::Decimal,
    processor::process_instruction,
    state::{Obligation, INITIAL_COLLATERAL_RATIO},
};

#[tokio::test]
async fn test_success() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(60_000);

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;
    const HOST_FEE_AMOUNT: u64 = 600_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    assert_eq!(
        usdc_reserve.liquidity.available_amount,
        initial_available_amount
    );

    let (total_fee, host_fee) = usdc_reserve
        .config
        .fees
        .calculate_flash_loan_fees(FLASH_LOAN_AMOUNT.into())
        .unwrap();
    assert_eq!(total_fee, FEE_AMOUNT);
    assert_eq!(host_fee, HOST_FEE_AMOUNT);

    let liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(liquidity_supply, initial_liquidity_supply);

    let token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(token_balance, initial_token_balance - FEE_AMOUNT);

    let fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.config.fee_receiver).await;
    assert_eq!(fee_balance, FEE_AMOUNT - HOST_FEE_AMOUNT);

    let host_fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_host_pubkey).await;
    assert_eq!(host_fee_balance, HOST_FEE_AMOUNT);
}

#[tokio::test]
async fn test_success_multiple_borrow_repays() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(120_000);

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;
    const HOST_FEE_AMOUNT: u64 = 600_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT * 2,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT * 2);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    assert_eq!(
        usdc_reserve.liquidity.available_amount,
        initial_available_amount
    );

    let (total_fee, host_fee) = usdc_reserve
        .config
        .fees
        .calculate_flash_loan_fees(FLASH_LOAN_AMOUNT.into())
        .unwrap();
    assert_eq!(total_fee, FEE_AMOUNT);
    assert_eq!(host_fee, HOST_FEE_AMOUNT);

    let liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(liquidity_supply, initial_liquidity_supply);

    let token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(token_balance, initial_token_balance - (FEE_AMOUNT * 2));

    let fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.config.fee_receiver).await;
    assert_eq!(fee_balance, (FEE_AMOUNT - HOST_FEE_AMOUNT) * 2);

    let host_fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_host_pubkey).await;
    assert_eq!(host_fee_balance, HOST_FEE_AMOUNT * 2);
}

#[tokio::test]
async fn test_fail_max_uint() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(60_000);

    const FLASH_LOAN_AMOUNT: u64 = u64::MAX;
    const LIQUIDITY_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: LIQUIDITY_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, LIQUIDITY_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, LIQUIDITY_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::InsufficientLiquidity as u32)
        )
    );
}

#[tokio::test]
async fn test_double_borrow() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::MultipleFlashBorrows as u32)
        )
    );
}

#[tokio::test]
async fn test_dont_repay() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[flash_borrow_reserve_liquidity(
            spl_token_lending::id(),
            FLASH_LOAN_AMOUNT,
            usdc_test_reserve.liquidity_supply_pubkey,
            usdc_test_reserve.user_liquidity_pubkey,
            usdc_test_reserve.pubkey,
            lending_market.pubkey,
        )],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::NoFlashRepayFound as u32)
        )
    );
}

#[tokio::test]
async fn test_dont_fully_repay() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT - 100, // Repay less than what was borrowed
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::InvalidFlashRepay as u32)
        )
    );
}

#[tokio::test]
async fn test_borrow_too_much() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    // Set liquidity amount too low for borrow
    const LIQUIDITY_AMOUNT: u64 = 500 * FRACTIONAL_TO_USDC;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: LIQUIDITY_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, LIQUIDITY_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, LIQUIDITY_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::InsufficientLiquidity as u32)
        )
    );
}

#[tokio::test]
async fn test_repay_in_wrong_token() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    test.prefer_bpf(false);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let sol_oracle = add_sol_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.config.fee_receiver,
                sol_test_reserve.liquidity_host_pubkey,
                sol_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::NoFlashRepayFound as u32)
        )
    );
}

#[tokio::test]
async fn test_cpi_borrow_fails() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    let flash_loan_proxy_program_account = Keypair::new();
    let flash_loan_proxy_program_id = flash_loan_proxy_program_account.pubkey();
    test.prefer_bpf(false);
    test.add_program(
        "flash_loan_proxy",
        flash_loan_proxy_program_id.clone(),
        processor!(helpers::flash_loan_proxy::process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let (lending_market_authority_pubkey, _) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &spl_token_lending::id(),
    );

    let mut transaction = Transaction::new_with_payer(
        &[helpers::flash_loan_proxy::borrow_proxy(
            flash_loan_proxy_program_id,
            FLASH_LOAN_AMOUNT,
            usdc_test_reserve.liquidity_supply_pubkey,
            usdc_test_reserve.user_liquidity_pubkey,
            usdc_test_reserve.pubkey,
            spl_token_lending::id(),
            lending_market.pubkey,
            lending_market_authority_pubkey,
        )],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::FlashBorrowCpi as u32)
        )
    );
}

#[tokio::test]
async fn test_cpi_repay_doesnt_count() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    let flash_loan_proxy_program_account = Keypair::new();
    let flash_loan_proxy_program_id = flash_loan_proxy_program_account.pubkey();
    test.prefer_bpf(false);
    test.add_program(
        "flash_loan_proxy",
        flash_loan_proxy_program_id.clone(),
        processor!(helpers::flash_loan_proxy::process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            helpers::flash_loan_proxy::repay_proxy(
                flash_loan_proxy_program_id,
                FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                spl_token_lending::id(),
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::NoFlashRepayFound as u32)
        )
    );
}

#[tokio::test]
async fn test_cpi_repay_fails() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    let flash_loan_proxy_program_account = Keypair::new();
    let flash_loan_proxy_program_id = flash_loan_proxy_program_account.pubkey();
    test.prefer_bpf(false);
    test.add_program(
        "flash_loan_proxy",
        flash_loan_proxy_program_id.clone(),
        processor!(helpers::flash_loan_proxy::process_instruction),
    );

    const FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const FEE_AMOUNT: u64 = 3_000_000;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.fees.flash_loan_fee_wad = 3_000_000_000_000_000;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT,
            liquidity_amount: FLASH_LOAN_AMOUNT,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(initial_liquidity_supply, FLASH_LOAN_AMOUNT);

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    let initial_available_amount = usdc_reserve.liquidity.available_amount;
    assert_eq!(initial_available_amount, FLASH_LOAN_AMOUNT);

    let initial_token_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(initial_token_balance, FEE_AMOUNT);

    let mut transaction = Transaction::new_with_payer(
        &[helpers::flash_loan_proxy::repay_proxy(
            flash_loan_proxy_program_id,
            FLASH_LOAN_AMOUNT,
            usdc_test_reserve.user_liquidity_pubkey,
            usdc_test_reserve.liquidity_supply_pubkey,
            usdc_test_reserve.config.fee_receiver,
            usdc_test_reserve.liquidity_host_pubkey,
            usdc_test_reserve.pubkey,
            spl_token_lending::id(),
            lending_market.pubkey,
            user_accounts_owner.pubkey(),
        )],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::FlashRepayCpi as u32)
        )
    );
}

#[tokio::test]
async fn end_to_end_with_flash_borrow() {
    let mut test = ProgramTest::new(
        "spl_token_lending",
        spl_token_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(250_000);

    const FEE_AMOUNT: u64 = 100;
    const HOST_FEE_AMOUNT: u64 = 20;
    const FLASH_LOAN_FEE_AMOUNT: u64 = 3_000_000;
    const FLASH_LOAN_HOST_FEE_AMOUNT: u64 = 600_000;

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = SOL_DEPOSIT_AMOUNT_LAMPORTS;

    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2_000 * FRACTIONAL_TO_USDC;
    const USDC_FLASH_LOAN_AMOUNT: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = (1_000 * FRACTIONAL_TO_USDC) - FEE_AMOUNT;
    const USDC_REPAY_AMOUNT_FRACTIONAL: u64 = USDC_RESERVE_LIQUIDITY_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_accounts_owner_pubkey = user_accounts_owner.pubkey();

    let user_transfer_authority = Keypair::new();
    let user_transfer_authority_pubkey = user_transfer_authority.pubkey();

    let obligation_keypair = Keypair::new();
    let obligation_pubkey = obligation_keypair.pubkey();

    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = test_reserve_config();
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            slots_elapsed: 1, // elapsed from 1; clock.slot = 2
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: FEE_AMOUNT + FLASH_LOAN_FEE_AMOUNT,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            slots_elapsed: 1, // elapsed from 1; clock.slot = 2
            ..AddReserveArgs::default()
        },
    );

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(3).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        last_blockhash: recent_blockhash,
        ..
    } = test_context;

    let payer_pubkey = payer.pubkey();

    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    let initial_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;

    let rent = banks_client.get_rent().await.unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[
            flash_borrow_reserve_liquidity(
                spl_token_lending::id(),
                USDC_FLASH_LOAN_AMOUNT,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
            ),
            // 0
            create_account(
                &payer.pubkey(),
                &obligation_keypair.pubkey(),
                rent.minimum_balance(Obligation::LEN),
                Obligation::LEN as u64,
                &spl_token_lending::id(),
            ),
            // 1
            init_obligation(
                spl_token_lending::id(),
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
            ),
            // 2
            refresh_reserve(
                spl_token_lending::id(),
                sol_test_reserve.pubkey,
                sol_oracle.pyth_price_pubkey,
                sol_oracle.switchboard_feed_pubkey,
            ),
            // 3
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            // 4
            deposit_obligation_collateral(
                spl_token_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
                user_transfer_authority_pubkey,
            ),
            // 5
            refresh_obligation(
                spl_token_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey],
            ),
            // 6
            refresh_reserve(
                spl_token_lending::id(),
                usdc_test_reserve.pubkey,
                usdc_oracle.pyth_price_pubkey,
                usdc_oracle.switchboard_feed_pubkey,
            ),
            // 7
            borrow_obligation_liquidity(
                spl_token_lending::id(),
                USDC_BORROW_AMOUNT_FRACTIONAL,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.config.fee_receiver,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
                Some(usdc_test_reserve.liquidity_host_pubkey),
            ),
            // 8
            refresh_reserve(
                spl_token_lending::id(),
                usdc_test_reserve.pubkey,
                usdc_oracle.pyth_price_pubkey,
                usdc_oracle.switchboard_feed_pubkey,
            ),
            // 9
            refresh_obligation(
                spl_token_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            // 10
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_REPAY_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            // 11
            repay_obligation_liquidity(
                spl_token_lending::id(),
                USDC_REPAY_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_transfer_authority_pubkey,
            ),
            // 12
            refresh_obligation(
                spl_token_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey],
            ),
            // 13
            withdraw_obligation_collateral(
                spl_token_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
            ),
            flash_repay_reserve_liquidity(
                spl_token_lending::id(),
                USDC_FLASH_LOAN_AMOUNT,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.config.fee_receiver,
                usdc_test_reserve.liquidity_host_pubkey,
                usdc_test_reserve.pubkey,
                lending_market.pubkey,
                user_accounts_owner.pubkey(),
            ),
        ],
        Some(&payer_pubkey),
    );

    transaction.sign(
        &vec![
            &payer,
            &obligation_keypair,
            &user_accounts_owner,
            &user_transfer_authority,
        ],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let usdc_reserve = usdc_test_reserve.get_state(&mut banks_client).await;

    let obligation = {
        let obligation_account: Account = banks_client
            .get_account(obligation_pubkey)
            .await
            .unwrap()
            .unwrap();
        Obligation::unpack(&obligation_account.data[..]).unwrap()
    };

    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(collateral_supply_balance, initial_collateral_supply_balance);
    assert_eq!(user_collateral_balance, initial_user_collateral_balance);

    let liquidity_supply =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    let user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(liquidity_supply, initial_liquidity_supply);
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - FEE_AMOUNT - FLASH_LOAN_FEE_AMOUNT
    );

    // If flash_borrow_reserve_liquidity did not force a reserve refresh,
    // then this line would fail as a borrowed_amount_wads would be non-zero
    // even though all obligations have been repaid
    assert_eq!(usdc_reserve.liquidity.borrowed_amount_wads, Decimal::zero());
    assert_eq!(
        usdc_reserve.liquidity.available_amount,
        initial_liquidity_supply
    );

    assert_eq!(obligation.deposits.len(), 0);
    assert_eq!(obligation.borrows.len(), 0);

    let fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.config.fee_receiver).await;
    assert_eq!(
        fee_balance,
        FLASH_LOAN_FEE_AMOUNT + FEE_AMOUNT - HOST_FEE_AMOUNT - FLASH_LOAN_HOST_FEE_AMOUNT
    );

    let host_fee_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_host_pubkey).await;
    assert_eq!(
        host_fee_balance,
        HOST_FEE_AMOUNT + FLASH_LOAN_HOST_FEE_AMOUNT
    );
}
