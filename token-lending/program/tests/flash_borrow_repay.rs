#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    instruction::InstructionError,
    pubkey::{Pubkey, PUBKEY_BYTES},
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
};
use spl_token_lending::{
    error::LendingError, instruction::flash_borrow_reserve_liquidity,
    instruction::flash_repay_reserve_liquidity, processor::process_instruction,
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
            1,
            InstructionError::Custom(LendingError::MultipleFlashBorrows as u32)
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
