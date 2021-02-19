use crate::error::BonfidaBotError;
use serum_dex::{
    instruction::SelfTradeBehavior,
    matching::{OrderType, Side},
};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use std::{
    convert::TryInto,
    mem::size_of,
    num::{NonZeroU16, NonZeroU64},
};

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub enum PoolInstruction {
    /// Initializes an empty pool account for the bonfida-bot program
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The system program account
    ///   1. `[]` The sysvar rent program account
    ///   2. `[]` The spl token program account
    ///   3. `[writable]` The pool account
    ///   4. `[writable]` The pooltoken mint account
    ///   5. `[writable, signer]` The fee payer account
    Init {
        // The seed used to derive the pool account
        pool_seed: [u8; 32],
        // The maximum number of token asset types the pool will ever be able to hold
        max_number_of_assets: u32,
        number_of_markets: u16
    },
    /// Creates a new pool from an empty (uninitialized) one by performing the first deposit
    /// of any number of different tokens and setting the pubkey of the signal provider.
    /// The first deposit will fix the initial value of 1 pooltoken (credited to the target)
    /// with respect to the deposited tokens.
    /// The init and create operations need to be separated as account data
    /// allocation needs to be first processed by the network before being overwritten.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[writable]` The pooltoken mint account
    ///   1. `[writable]` The target account that receives the pooltokens
    ///   0. `[writable]` The pool account
    ///   2..M+2. `[writable]` The M pool (associated) token assets accounts in the order of the
    ///      corresponding PoolAssets in the pool account data.
    ///   M+3. `[signer]` The source owner account
    ///   M+4..2M+4. `[writable]` The M source token accounts in the same order as above
    Create {
        pool_seed: [u8; 32],
        serum_program_id: Pubkey,
        signal_provider_key: Pubkey,
        deposit_amounts: Vec<u64>,
        markets: Vec<Pubkey>
    },
    /// Buy into the pool. The source deposits tokens into the pool and the target receives
    /// a corresponding amount of pool-token in exchange. The program will try to
    /// maximize the deposit sum with regards to the amounts given by the source and
    /// the ratio of tokens present in the pool at that moment. Tokens can only be deposited
    /// in the exact ratio of tokens that are present in the pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[writable]` The pooltoken mint account
    ///   1. `[writable]` The target account that receives the pooltokens
    ///   1. `[]` The pool account
    ///   2..M+2. `[writable]` The M pool (associated) token assets accounts in the order of the
    ///      corresponding PoolAssets in the pool account data.
    ///   M+3. `[signer]` The source owner account
    ///   M+4..2M+4. `[writable]` The M source token accounts in the same order as above
    Deposit {
        pool_seed: [u8; 32],
        // The amount of pool token the source wishes to buy
        pool_token_amount: u64,
    },
    /// As a signal provider, create a new serum order for the pool.
    /// Amounts are translated into proportions of the pool between 0 and 2**16 - 1
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///    0. `[signer]` The signal provider account
    ///    1. `[writable]` The market account
    ///    2. `[writable]` The payer pool asset account
    ///    3. `[writable]` The relevant OpenOrders account
    ///    5. `[writable]` The Serum request queue
    ///    6. `[writable]` The pool account
    ///    7. `[writable]` The coin vault
    ///    8. `[writable]` The price currency vault
    ///    9. `[]` The spl_token_program
    ///   10. `[]` The rent sysvar account
    ///   11. `[]` The dex program account
    ///   12. `[writable]` (optional) The (M)SRM referrer account
    CreateOrder {
        pool_seed: [u8; 32],
        side: Side,
        limit_price: NonZeroU64,
        ratio_of_pool_assets_to_trade: NonZeroU16,
        order_type: OrderType,
        client_id: u64,
        self_trade_behavior: SelfTradeBehavior,
        source_index: u64,
        target_index: u64,
        market_index: u16,
        coin_lot_size: u64,
        pc_lot_size: u64,
        target_mint: Pubkey,
    },
    /// As a signal provider, cancel a serum order for the pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///    0. `[signer]` The signal provider account
    ///    1. `[]` The market account
    ///    2. `[writable]` The relevant OpenOrders account
    ///    3. `[writable]` The Serum request queue
    ///    4. `[]` The pool account
    ///    5. `[]` The dex program account
    CancelOrder {
        pool_seed: [u8; 32],
        side: Side,
        order_id: u128,
    },
    /// A permissionless crank to settle funds out of one of the pool's active OpenOrders accounts.
    ///
    /// Accounts expected by this instruction:
    ///
    ///    0. `[writable]` The market account
    ///    1. `[writable]` The pool's OpenOrders account
    ///    3. `[writable]` the pool account
    ///    4. `[]` the pool token mint
    ///    5. `[writable]` coin vault
    ///    6. `[writable]` pc vault
    ///    7. `[writable]` the pool coin wallet
    ///    8. `[writable]` the pool pc wallet
    ///    9. `[]` vault signer
    ///   10. `[]` spl token program
    ///   11. `[]` Serum dex program
    ///   12. `[writable]` (optional) referrer pc wallet
    SettleFunds {
        pool_seed: [u8; 32],
        pc_index: u64,
        coin_index: u64,
    },
    /// Buy out of the pool by redeeming pooltokens.
    /// This instruction needs to be executed after (and within the same transaction)
    /// having settled on all possible open orders for the pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[writable]` The pooltoken mint account
    ///   2. `[signer]` The pooltoken source account owner
    ///   3. `[writable]` The pooltoken source account
    ///   4. `[]` The pool account
    ///   5..M+5. `[writable]` The M pool (associated) token assets accounts in the order of the
    ///      corresponding PoolAssets found in the pool account data.
    ///   M+6..2M+6. `[writable]` The M target token accounts in the same order as above
    Redeem {
        pool_seed: [u8; 32],
        // The amount of pool token the source wishes to redeem
        pool_token_amount: u64,
    },
}

impl PoolInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        use BonfidaBotError::InvalidInstruction;
        let (&tag, rest) = input.split_first().ok_or(InvalidInstruction)?;
        Ok(match tag {
            0 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let max_number_of_assets: u32 = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let number_of_markets: u16 = rest
                    .get(36..38)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u16::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Init {
                    pool_seed,
                    max_number_of_assets,
                    number_of_markets
                }
            }
            1 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let serum_program_id = rest
                    .get(32..64)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let signal_provider_key = rest
                    .get(64..96)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let number_of_markets = rest
                    .get(96..98)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u16::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let mut markets = Vec::with_capacity(number_of_markets as usize);
                let mut offset = 98;
                for _ in 0..number_of_markets {
                    markets.push(rest
                        .get(offset..offset + 32)
                        .and_then(|slice| slice.try_into().ok())
                        .map(Pubkey::new)
                        .ok_or(InvalidInstruction)?
                    );
                    offset = offset + 32;
                }
                let mut k = offset;
                let mut deposit_amounts = vec![];
                while k != 0 {
                    match rest.get(k..(k + 8)) {
                        None => k = 0,
                        Some(bytes) => {
                            deposit_amounts.push(u64::from_le_bytes(bytes.try_into().unwrap()));
                            k = k + 8;
                        }
                    }
                }
                Self::Create {
                    pool_seed,
                    serum_program_id,
                    signal_provider_key,
                    markets,
                    deposit_amounts,
                }
            }
            2 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let pool_token_amount = rest
                    .get(32..40)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Deposit {
                    pool_seed,
                    pool_token_amount,
                }
            }
            3 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .ok_or(InvalidInstruction)?;
                let side = match rest.get(32).ok_or(InvalidInstruction)? {
                    0 => Side::Bid,
                    1 => Side::Ask,
                    _ => return Err(InvalidInstruction.into()),
                };
                let limit_price = NonZeroU64::new(
                    rest.get(33..41)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u64::from_le_bytes)
                        .ok_or(InvalidInstruction)?,
                )
                .ok_or(InvalidInstruction)?;
                let ratio_of_pool_assets_to_trade = NonZeroU16::new(
                    rest.get(41..43)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u16::from_le_bytes)
                        .ok_or(InvalidInstruction)?,
                )
                .ok_or(InvalidInstruction)?;

                let order_type = match rest.get(43).ok_or(InvalidInstruction)? {
                    0 => OrderType::Limit,
                    1 => OrderType::ImmediateOrCancel,
                    2 => OrderType::PostOnly,
                    _ => return Err(InvalidInstruction.into()),
                };
                let client_id = rest
                    .get(44..52)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let self_trade_behavior = match rest.get(52).ok_or(InvalidInstruction)? {
                    0 => SelfTradeBehavior::DecrementTake,
                    1 => SelfTradeBehavior::CancelProvide,
                    _ => return Err(InvalidInstruction.into()),
                };
                let source_index = rest
                    .get(53..61)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let target_index = rest
                    .get(61..69)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let market_index = rest
                    .get(69..71)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u16::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let coin_lot_size = rest
                    .get(71..79)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let pc_lot_size = rest
                    .get(79..87)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let target_mint = rest
                    .get(87..119)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                Self::CreateOrder {
                    pool_seed,
                    side,
                    limit_price,
                    ratio_of_pool_assets_to_trade,
                    order_type,
                    client_id,
                    self_trade_behavior,
                    source_index,
                    target_index,
                    market_index,
                    coin_lot_size,
                    pc_lot_size,
                    target_mint,
                }
            }
            4 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let side = match rest.get(32).ok_or(InvalidInstruction)? {
                    0 => Side::Bid,
                    1 => Side::Ask,
                    _ => return Err(InvalidInstruction.into()),
                };
                let order_id = rest
                    .get(33..49)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u128::from_le_bytes)
                    .ok_or(InvalidInstruction)?;

                Self::CancelOrder {
                    pool_seed,
                    side,
                    order_id,
                }
            }
            5 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let pc_index = rest
                    .get(32..40)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let coin_index = rest
                    .get(40..48)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::SettleFunds {
                    pool_seed,
                    pc_index,
                    coin_index,
                }
            }
            6 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let pool_token_amount = rest
                    .get(32..40)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Redeem {
                    pool_seed,
                    pool_token_amount,
                }
            }
            _ => {
                msg!("Unsupported tag");
                return Err(InvalidInstruction.into());
            }
        })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match self {
            Self::Init {
                pool_seed,
                max_number_of_assets,
                number_of_markets
            } => {
                buf.push(0);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&max_number_of_assets.to_le_bytes());
                buf.extend_from_slice(&number_of_markets.to_le_bytes());
            }
            Self::Create {
                pool_seed,
                serum_program_id,
                signal_provider_key,
                deposit_amounts,
                markets
            } => {
                buf.push(1);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&serum_program_id.to_bytes());
                buf.extend_from_slice(&signal_provider_key.to_bytes());
                buf.extend_from_slice(&(markets.len() as u16).to_le_bytes());
                for market in markets {
                    buf.extend_from_slice(&market.to_bytes())
                }
                for amount in deposit_amounts.iter() {
                    buf.extend_from_slice(&amount.to_le_bytes());
                }
            }
            Self::Deposit {
                pool_seed,
                pool_token_amount,
            } => {
                buf.push(2);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&pool_token_amount.to_le_bytes());
            }
            Self::CreateOrder {
                pool_seed,
                side,
                limit_price,
                ratio_of_pool_assets_to_trade,
                order_type,
                client_id,
                self_trade_behavior,
                source_index,
                target_index,
                market_index,
                coin_lot_size,
                pc_lot_size,
                target_mint,
            } => {
                buf.push(3);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(
                    &match side {
                        Side::Bid => 0u8,
                        Side::Ask => 1,
                    }
                    .to_le_bytes(),
                );
                buf.extend_from_slice(&limit_price.get().to_le_bytes());
                buf.extend_from_slice(&ratio_of_pool_assets_to_trade.get().to_le_bytes());
                buf.extend_from_slice(
                    &match order_type {
                        OrderType::Limit => 0u8,
                        OrderType::ImmediateOrCancel => 1,
                        OrderType::PostOnly => 2,
                    }
                    .to_le_bytes(),
                );
                buf.extend_from_slice(&client_id.to_le_bytes());
                buf.extend_from_slice(
                    &match self_trade_behavior {
                        SelfTradeBehavior::DecrementTake => 0u8,
                        SelfTradeBehavior::CancelProvide => 1,
                    }
                    .to_le_bytes(),
                );
                buf.extend_from_slice(&source_index.to_le_bytes());
                buf.extend_from_slice(&target_index.to_le_bytes());
                buf.extend_from_slice(&market_index.to_le_bytes());
                buf.extend_from_slice(&coin_lot_size.to_le_bytes());
                buf.extend_from_slice(&pc_lot_size.to_le_bytes());
                buf.extend_from_slice(&target_mint.to_bytes());
            }
            Self::CancelOrder {
                pool_seed,
                side,
                order_id,
            } => {
                buf.push(4);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(
                    &match side {
                        Side::Bid => 0u8,
                        Side::Ask => 1,
                    }
                    .to_le_bytes(),
                );
                buf.extend_from_slice(&order_id.to_le_bytes());
            }
            Self::SettleFunds {
                pool_seed,
                pc_index,
                coin_index,
            } => {
                buf.push(5);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&pc_index.to_le_bytes());
                buf.extend_from_slice(&coin_index.to_le_bytes());
            }
            Self::Redeem {
                pool_seed,
                pool_token_amount,
            } => {
                buf.push(6);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&pool_token_amount.to_le_bytes());
            }
        };
        buf
    }
}

// Creates a `Init` instruction
pub fn init(
    spl_token_program_id: &Pubkey,
    system_program_id: &Pubkey,
    rent_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    mint_key: &Pubkey,
    payer_key: &Pubkey,
    pool_key: &Pubkey,
    pool_seed: [u8; 32],
    max_number_of_assets: u32,
    number_of_markets: u16
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Init {
        pool_seed,
        max_number_of_assets,
        number_of_markets
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*system_program_id, false),
        AccountMeta::new_readonly(*rent_program_id, false),
        AccountMeta::new_readonly(*spl_token_program_id, false),
        AccountMeta::new(*pool_key, false),
        AccountMeta::new(*mint_key, false),
        AccountMeta::new(*payer_key, true),
    ];
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a `CreatePool` instruction
pub fn create(
    spl_token_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    mint_key: &Pubkey,
    pool_key: &Pubkey,
    pool_seed: [u8; 32],
    pool_asset_keys: &Vec<Pubkey>,
    target_pool_token_key: &Pubkey,
    source_owner_key: &Pubkey,
    source_asset_keys: &Vec<Pubkey>,
    serum_program_id: &Pubkey,
    signal_provider_key: &Pubkey,
    deposit_amounts: Vec<u64>,
    markets: Vec<Pubkey>
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Create {
        pool_seed,
        serum_program_id: *serum_program_id,
        signal_provider_key: *signal_provider_key,
        deposit_amounts,
        markets
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new_readonly(*spl_token_program_id, false),
        AccountMeta::new(*mint_key, false),
        AccountMeta::new(*target_pool_token_key, false),
        AccountMeta::new(*pool_key, false),
    ];
    for pool_asset_key in pool_asset_keys.iter() {
        accounts.push(AccountMeta::new(*pool_asset_key, false))
    }
    accounts.push(AccountMeta::new_readonly(*source_owner_key, true));
    for source_asset_key in source_asset_keys.iter() {
        accounts.push(AccountMeta::new(*source_asset_key, false))
    }

    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a `Deposit` instruction
pub fn deposit(
    spl_token_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    mint_key: &Pubkey,
    pool_key: &Pubkey,
    pool_asset_keys: &Vec<Pubkey>,
    target_pool_token_key: &Pubkey,
    source_owner: &Pubkey,
    source_asset_keys: &Vec<Pubkey>,
    pool_seed: [u8; 32],
    pool_token_amount: u64,
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Deposit {
        pool_seed,
        pool_token_amount,
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new_readonly(*spl_token_program_id, false),
        AccountMeta::new(*mint_key, false),
        AccountMeta::new(*target_pool_token_key, false),
        AccountMeta::new_readonly(*pool_key, false),
    ];
    for pool_asset_key in pool_asset_keys.iter() {
        accounts.push(AccountMeta::new(*pool_asset_key, false))
    }
    accounts.push(AccountMeta::new_readonly(*source_owner, true));
    for source_asset_key in source_asset_keys.iter() {
        accounts.push(AccountMeta::new(*source_asset_key, false))
    }
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a `Redeem` instruction
pub fn redeem(
    spl_token_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    mint_key: &Pubkey,
    pool_key: &Pubkey,
    pool_asset_keys: &Vec<Pubkey>,
    source_pool_token_owner_key: &Pubkey,
    source_pool_token_key: &Pubkey,
    target_asset_keys: &Vec<Pubkey>,
    pool_seed: [u8; 32],
    pool_token_amount: u64,
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Redeem {
        pool_seed,
        pool_token_amount,
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new_readonly(*spl_token_program_id, false),
        AccountMeta::new(*mint_key, false),
        AccountMeta::new_readonly(*source_pool_token_owner_key, true),
        AccountMeta::new(*source_pool_token_key, false),
        AccountMeta::new(*pool_key, false),
    ];
    for pool_asset_key in pool_asset_keys.iter() {
        accounts.push(AccountMeta::new(*pool_asset_key, false))
    }
    for source_asset_key in target_asset_keys.iter() {
        accounts.push(AccountMeta::new(*source_asset_key, false))
    }
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a `CreateOrder` instruction
pub fn create_order(
    bonfidabot_program_id: &Pubkey,
    signal_provider: &Pubkey,
    market: &Pubkey,
    payer_pool_asset_account: &Pubkey,
    payer_pool_asset_index: u64,
    target_pool_asset_index: u64,
    openorders_account: &Pubkey,
    serum_request_queue: &Pubkey,
    pool_account: &Pubkey,
    coin_vault: &Pubkey,
    pc_vault: &Pubkey,
    spl_token_program: &Pubkey,
    dex_program: &Pubkey,
    rent_sysvar: &Pubkey,
    srm_referrer_account: Option<&Pubkey>,
    pool_seed: [u8; 32],
    side: Side,
    limit_price: NonZeroU64,
    market_index: u16,
    coin_lot_size: u64,
    pc_lot_size: u64,
    target_mint: &Pubkey,
    ratio_of_pool_assets_to_trade: NonZeroU16,
    order_type: OrderType,
    client_id: u64,
    self_trade_behavior: SelfTradeBehavior,
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::CreateOrder {
        pool_seed,
        side,
        limit_price,
        ratio_of_pool_assets_to_trade,
        order_type,
        client_id,
        self_trade_behavior,
        source_index: payer_pool_asset_index,
        target_index: target_pool_asset_index,
        market_index,
        coin_lot_size,
        pc_lot_size,
        target_mint: *target_mint,
        
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new_readonly(*signal_provider, true),
        AccountMeta::new(*market, false),
        AccountMeta::new(*payer_pool_asset_account, false),
        AccountMeta::new(*openorders_account, false),
        AccountMeta::new(*serum_request_queue, false),
        AccountMeta::new(*pool_account, false),
        AccountMeta::new(*coin_vault, false),
        AccountMeta::new(*pc_vault, false),
        AccountMeta::new_readonly(*spl_token_program, false),
        AccountMeta::new_readonly(*rent_sysvar, false),
        AccountMeta::new_readonly(*dex_program, false),
    ];
    if let Some(key) = srm_referrer_account {
        accounts.push(AccountMeta::new(*key, false));
    }
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a `CancelOrder` instruction
pub fn cancel_order(
    bonfidabot_program_id: &Pubkey,
    signal_provider: &Pubkey,
    market: &Pubkey,
    openorders_account: &Pubkey,
    serum_request_queue: &Pubkey,
    pool_account: &Pubkey,
    dex_program: &Pubkey,
    pool_seed: [u8; 32],
    side: Side,
    order_id: u128,
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::CancelOrder {
        pool_seed,
        side,
        order_id,
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*signal_provider, true),
        AccountMeta::new_readonly(*market, false),
        AccountMeta::new(*openorders_account, false),
        AccountMeta::new(*serum_request_queue, false),
        AccountMeta::new_readonly(*pool_account, false),
        AccountMeta::new_readonly(*dex_program, false),
    ];
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

// Creates a settle funds
pub fn settle_funds(
    bonfidabot_program_id: &Pubkey,
    market: &Pubkey,
    openorders_account: &Pubkey,
    pool_account: &Pubkey,
    pool_token_mint: &Pubkey,
    coin_vault: &Pubkey,
    pc_vault: &Pubkey,
    pool_coin_wallet: &Pubkey,
    pool_pc_wallet: &Pubkey,
    vault_signer: &Pubkey,
    spl_token_program: &Pubkey,
    dex_program: &Pubkey,
    referrer_pc_account: Option<&Pubkey>,
    pool_seed: [u8; 32],
    pc_index: u64,
    coin_index: u64,
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::SettleFunds {
        pool_seed,
        pc_index,
        coin_index,
    }
    .pack();

    let mut accounts = vec![
        AccountMeta::new(*market, false),
        AccountMeta::new(*openorders_account, false),
        AccountMeta::new(*pool_account, false),
        AccountMeta::new_readonly(*pool_token_mint, false),
        AccountMeta::new(*coin_vault, false),
        AccountMeta::new(*pc_vault, false),
        AccountMeta::new(*pool_coin_wallet, false),
        AccountMeta::new(*pool_pc_wallet, false),
        AccountMeta::new_readonly(*vault_signer, false),
        AccountMeta::new_readonly(*spl_token_program, false),
        AccountMeta::new_readonly(*dex_program, false),
    ];
    if let Some(key) = referrer_pc_account {
        accounts.push(AccountMeta::new(*key, false))
    }
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

#[cfg(test)]
mod test {
    use std::num::{NonZeroU16, NonZeroU64};

    use serum_dex::{
        instruction::SelfTradeBehavior,
        matching::{OrderType, Side},
    };
    use solana_program::pubkey::Pubkey;

    use super::PoolInstruction;

    #[test]
    fn test_instruction_packing() {
        let original_init = PoolInstruction::Init {
            pool_seed: [50u8; 32],
            max_number_of_assets: 43,
            number_of_markets: 50
        };
        assert_eq!(
            original_init,
            PoolInstruction::unpack(&original_init.pack()).unwrap()
        );

        let original_create = PoolInstruction::Create {
            pool_seed: [50u8; 32],
            serum_program_id: Pubkey::new_unique(),
            signal_provider_key: Pubkey::new_unique(),
            deposit_amounts: vec![23 as u64, 43 as u64],
            markets: vec![Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique()]
        };
        let packed_create = original_create.pack();
        let unpacked_create = PoolInstruction::unpack(&packed_create).unwrap();
        assert_eq!(original_create, unpacked_create);

        let original_deposit = PoolInstruction::Deposit {
            pool_seed: [50u8; 32],
            pool_token_amount: 24 as u64,
        };
        let packed_deposit = original_deposit.pack();
        let unpacked_deposit = PoolInstruction::unpack(&packed_deposit).unwrap();
        assert_eq!(original_deposit, unpacked_deposit);

        let original_create_order = PoolInstruction::CreateOrder {
            pool_seed: [50u8; 32],
            side: Side::Ask,
            limit_price: NonZeroU64::new(23).unwrap(),
            ratio_of_pool_assets_to_trade: NonZeroU16::new(500).unwrap(),
            order_type: OrderType::Limit,
            client_id: 0xff44,
            self_trade_behavior: SelfTradeBehavior::DecrementTake,
            source_index: 42,
            target_index: 78,
            market_index: 41,
            coin_lot_size: 41,
            pc_lot_size: 41,
            target_mint: Pubkey::new_unique(),
        };
        let packed_create_order = original_create_order.pack();
        let unpacked_create_order = PoolInstruction::unpack(&packed_create_order).unwrap();
        assert_eq!(original_create_order, unpacked_create_order);
        assert_eq!(original_deposit, unpacked_deposit);

        let original_settle_order = PoolInstruction::SettleFunds {
            pool_seed: [50u8; 32],
            pc_index: 42,
            coin_index: 52,
        };
        let packed_settle_order = original_settle_order.pack();
        let unpacked_settle_order = PoolInstruction::unpack(&packed_settle_order).unwrap();
        assert_eq!(original_settle_order, unpacked_settle_order);

        let original_redeem = PoolInstruction::Redeem {
            pool_seed: [50u8; 32],
            pool_token_amount: 24 as u64,
        };
        let packed_redeem = original_redeem.pack();
        let unpacked_redeem = PoolInstruction::unpack(&packed_redeem).unwrap();
        assert_eq!(original_redeem, unpacked_redeem);

        let original_cancel_order = PoolInstruction::CancelOrder {
            pool_seed: [50u8; 32],
            side: Side::Ask,
            order_id: 855464984,
        };
        let packed_cancel_order = original_cancel_order.pack();
        let unpacked_cancel_order = PoolInstruction::unpack(&packed_cancel_order).unwrap();
        assert_eq!(original_cancel_order, unpacked_cancel_order);
    }
}
