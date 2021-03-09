import {
  Account,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  Connection,
  TokenAmount,
  ConfirmedSignatureInfo,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout, u64 } from '@solana/spl-token';
import { Market, TOKEN_MINTS, MARKETS, OpenOrders } from '@project-serum/serum';
import { depositInstruction } from './instructions';
import {
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  Numberu64,
  getMarketData,
  getMidPrice,
  signAndSendTransactionInstructions,
  sleep,
  findAndCreateAssociatedAccount,
  Numberu16,
} from './utils';
import {
  OrderSide,
  OrderType,
  PoolHeader,
  PoolStatus,
  PUBKEY_LENGTH,
  SelfTradeBehavior,
  unpack_assets,
  unpack_markets,
} from './state';
import { PoolAssetBalance, PoolOrderInfo } from './types';
import {
  BONFIDABOT_PROGRAM_ID,
  BONFIDA_BNB_KEY,
  BONFIDA_FEE_KEY,
  createPool,
  SERUM_PROGRAM_ID,
  settleFunds
} from './main';
import { connect } from 'http2';
import Wallet from '@project-serum/sol-wallet-adapter';

export type PoolInfo = {
  address: PublicKey;
  serumProgramId: PublicKey;
  seed: Uint8Array;
  signalProvider: PublicKey;
  status: PoolStatus;
  feeRatio: Numberu16;
  feePeriod: Numberu64;
  mintKey: PublicKey;
  assetMintkeys: Array<PublicKey>;
  authorizedMarkets: Array<PublicKey>;
};

// TODO singleTokenDeposit optim + singleTokenRedeem


/**
 * Returns the solana instructions to settle all open orders for a given pool.
 * If the returned transaction array is too large for it to be sent on Solana, 
 * you may need to process it batch-wise.
 * 
 * @param connection 
 * @param poolSeed 
 */
export async function settlePool(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
): Promise<TransactionInstruction[]> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolData.data.slice(0, PoolHeader.LEN),
  );
  
  let authorizedMarkets = unpack_markets(
    poolData.data.slice(
      PoolHeader.LEN,
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
    poolHeader.numberOfMarkets,
  );

  let instructions: TransactionInstruction[] = [];
  for (let authorizedMarket of authorizedMarkets) {
    const market = await Market.load(
      connection,
      authorizedMarket,
      {},
      SERUM_PROGRAM_ID,
    );
  
    const openOrdersAccounts = await market.findOpenOrdersAccountsForOwner(
      connection,
      poolKey,
    );
    console.log(openOrdersAccounts.length);
    for (let openOrder of openOrdersAccounts) {
      instructions.push((await settleFunds(
        connection,
        poolSeed,
        authorizedMarket,
        openOrder.address,
        null
      ))[0]); 
    }
  }
  return instructions;
}

/**
 * Returns a structure containing most informations that one can parse from a pools state.
 * @param connection 
 * @param poolSeed 
 */
export async function fetchPoolInfo(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
): Promise<PoolInfo> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolData.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolData.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let authorizedMarkets = unpack_markets(
    poolData.data.slice(
      PoolHeader.LEN,
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
    poolHeader.numberOfMarkets,
  );

  let poolInfo: PoolInfo = {
    address: poolKey,
    serumProgramId: poolHeader.serumProgramId,
    seed: poolHeader.seed,
    signalProvider: poolHeader.signalProvider,
    status: poolHeader.status,
    feeRatio: poolHeader.feeRatio,
    feePeriod: poolHeader.feeCollectionPeriod,
    mintKey: poolMintKey,
    assetMintkeys: poolAssets.map(asset => asset.mintAddress),
    authorizedMarkets,
  };

  return poolInfo;
}

/**
 * Fetch the balances of the poolToken and the assets (returned in the same order as in the poolData)
 * 
 * @param connection 
 * @param poolSeed
 */ 
export async function fetchPoolBalances(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
): Promise<[TokenAmount, Array<PoolAssetBalance>]> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolData.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolData.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let assetBalances: Array<PoolAssetBalance> = [];
  for (let asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    let balance = (await connection.getTokenAccountBalance(assetKey)).value;
    assetBalances.push({
      tokenAmount: balance,
      mint: asset.mintAddress.toBase58(),
    });
  }

  let poolTokenSupply = (await connection.getTokenSupply(poolMintKey)).value;

  return [poolTokenSupply, assetBalances];
}


/**
 * This method lets the user deposit an arbitrary token into the pool
 * by intermediately trading with serum in order to reach the pool asset ratio.
 * (WIP)
 * 
 * @param connection 
 * @param sourceOwner 
 * @param sourceTokenKey 
 * @param user_amount 
 * @param poolSeed 
 * @param payer 
 */
export async function singleTokenDeposit(
  connection: Connection,
  sourceOwner: Wallet,
  sourceTokenKey: PublicKey,
  // The amount of source tokens to invest into pool
  user_amount: number,
  poolSeed: Buffer | Uint8Array,
  payer: Account,
) {
  // Fetch Poolasset mints
  console.log('Creating source asset accounts');
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolInfo.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolInfo.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  // Transfer source tokens to USDC
  let tokenInfo = await connection.getAccountInfo(sourceTokenKey);
  if (!tokenInfo) {
    throw 'Source asset account is unavailable';
  }
  let tokenData = Buffer.from(tokenInfo.data);
  const tokenMint = new PublicKey(AccountLayout.decode(tokenData).mint);
  const tokenInitialBalance: number = AccountLayout.decode(tokenData).amount;
  let tokenSymbol = TOKEN_MINTS[
    TOKEN_MINTS.map(t => t.address.toString()).indexOf(tokenMint.toString())
  ].name;
  let precision = await (await connection.getTokenAccountBalance(sourceTokenKey)).value.decimals;
  let amount = precision * user_amount;

  let midPriceUSDC: number, sourceUSDCKey: PublicKey;
  if (tokenSymbol != 'USDC') {
    let pairSymbol = tokenSymbol.concat('/USDC');
    let usdcMarketInfo =
      MARKETS[
        MARKETS.map(m => {
          return m.name;
        }).lastIndexOf(pairSymbol)
      ];
    if (usdcMarketInfo.deprecated) {
      throw 'Chosen Market is deprecated';
    }

    let marketUSDC: Market;
    [marketUSDC, midPriceUSDC] = await getMidPrice(
      connection,
      usdcMarketInfo.address,
    );

    console.log(tokenInitialBalance);
    console.log('Creating token to USDC order');
    console.log({
      owner: sourceOwner.publicKey.toString(),
      payer: sourceTokenKey.toString(),
      side: 'sell',
      price: 0.95 * midPriceUSDC,
      size: amount,
      orderType: 'ioc',
    });
    await marketUSDC.placeOrder(connection, {
      owner: sourceOwner,
      payer: sourceTokenKey,
      side: 'sell',
      price: 0.95 * midPriceUSDC,
      size: amount,
      orderType: 'ioc',
    });

    sourceUSDCKey = await findAssociatedTokenAddress(
      sourceOwner.publicKey,
      marketUSDC.quoteMintAddress,
    );
    let sourceUSDCInfo = await connection.getAccountInfo(sourceUSDCKey);
    if (!sourceUSDCInfo) {
      let createUSDCInstruction = await createAssociatedTokenAccount(
        SystemProgram.programId,
        payer.publicKey,
        sourceOwner.publicKey,
        marketUSDC.quoteMintAddress,
      );
      await signAndSendTransactionInstructions(connection, [], payer, [
        createUSDCInstruction,
      ]);
    }

    // Wait for the Serum Event Queue to be processed
    await sleep(3 * 1000);

    // Settle the sourceToken to USDC transfer
    console.log('Settling order');
    let openOrders = await marketUSDC.findOpenOrdersAccountsForOwner(
      connection,
      sourceOwner.publicKey,
    );
    for (let openOrder of openOrders) {
      await marketUSDC.settleFunds(
        connection,
        sourceOwner,
        openOrder,
        sourceTokenKey,
        sourceUSDCKey,
      );
    }
  } else {
    midPriceUSDC = 1;
    sourceUSDCKey = sourceTokenKey;
  }

  // Verify that order went through correctly
  tokenInfo = await connection.getAccountInfo(sourceTokenKey);
  if (!tokenInfo) {
    throw 'Source asset account is unavailable';
  }
  tokenData = Buffer.from(tokenInfo.data);
  let tokenBalance = AccountLayout.decode(tokenData).amount;
  if (tokenInitialBalance - tokenBalance > amount) {
    throw 'Conversion to USDC Order was not matched.';
  }

  // Create the source asset account if nonexistent
  let createAssetInstructions: TransactionInstruction[] = new Array();
  let sourceAssetKeys: Array<PublicKey> = [];
  let poolAssetKeys: Array<PublicKey> = [];
  for (let asset of poolAssets) {
    let sourceAssetKey = await findAssociatedTokenAddress(
      sourceOwner.publicKey,
      asset.mintAddress,
    );
    sourceAssetKeys.push(sourceAssetKey);
    let poolAssetKey = await findAssociatedTokenAddress(
      poolKey,
      asset.mintAddress,
    );
    poolAssetKeys.push(poolAssetKey);
    let sourceAssetInfo = await connection.getAccountInfo(sourceAssetKey);
    if (!sourceAssetInfo) {
      createAssetInstructions.push(
        await createAssociatedTokenAccount(
          SystemProgram.programId,
          payer.publicKey,
          sourceOwner.publicKey,
          asset.mintAddress,
        ),
      );
    }
  }
  if (createAssetInstructions.length > 0) {
    await signAndSendTransactionInstructions(
      connection,
      [],
      payer,
      createAssetInstructions,
    );
  }

  // Buy the corresponding tokens with the source USDC in correct ratios
  console.log('Invest USDC in pool ratios');
  let totalPoolAssetAmount: number = 0;
  let poolAssetAmounts: Array<number> = [];
  for (let asset of poolAssets) {
    let poolAssetKey = await findAssociatedTokenAddress(
      poolKey,
      asset.mintAddress,
    );
    let poolAssetBalance = +(
      await connection.getTokenAccountBalance(poolAssetKey)
    ).value.amount;
    poolAssetAmounts.push(poolAssetBalance);
    totalPoolAssetAmount += poolAssetBalance;
  }
  let poolAssetMarkets: Array<Market | undefined> = [];
  let poolTokenAmount = 0;
  for (let i = 0; i < poolAssets.length; i++) {
    let poolAssetSymbol =
      TOKEN_MINTS[
        TOKEN_MINTS.map(t => t.address.toString()).indexOf(
          poolAssets[i].mintAddress.toString(),
        )
      ].name;
    if (poolAssetSymbol != 'USDC') {
      let assetPairSymbol = poolAssetSymbol.concat('/USDC');

      let assetMarketInfo =
        MARKETS[
          MARKETS.map(m => {
            return m.name;
          }).lastIndexOf(assetPairSymbol)
        ];
      if (assetMarketInfo.deprecated) {
        throw 'Chosen Market is deprecated';
      }

      if (poolAssetAmounts[i] == 0) {
        continue
      }

      let [assetMarket, assetMidPrice] = await getMidPrice(
        connection,
        assetMarketInfo.address,
      );
      poolAssetMarkets.push(assetMarket);
      let assetAmountToBuy =
        (midPriceUSDC * amount * poolAssetAmounts[i]) /
        (assetMidPrice * totalPoolAssetAmount);
      poolTokenAmount = Math.max(
        poolTokenAmount,
        assetAmountToBuy / poolAssetAmounts[i],
      );
      console.log(assetPairSymbol);
      console.log({
        owner: sourceOwner.publicKey.toString(),
        payer: sourceUSDCKey.toString(),
        side: 'buy',
        price: 1.05 * assetMidPrice,
        size: assetAmountToBuy,
        orderType: 'ioc',
      });
      await assetMarket.placeOrder(connection, {
        owner: sourceOwner,
        payer: sourceUSDCKey,
        side: 'buy',
        price: 1.05 * assetMidPrice,
        size: assetAmountToBuy,
        orderType: 'ioc',
      });
    } else {
      poolAssetMarkets.push(undefined);
      poolTokenAmount = Math.max(
        poolTokenAmount,
        (1000000 * midPriceUSDC * amount) / totalPoolAssetAmount,
      );
    }
  }

  // Wait for the Serum Event Queue to be processed
  await sleep(3 * 1000);

  // Settle the USDC to Poolassets transfers
  console.log('Settling the orders');
  for (let i = 0; i < poolAssets.length; i++) {
    let assetMarket = poolAssetMarkets[i];
    if (!!assetMarket) {
      let openOrders = await assetMarket.findOpenOrdersAccountsForOwner(
        connection,
        sourceOwner.publicKey,
      );
      for (let openOrder of openOrders) {
        await assetMarket.settleFunds(
          connection,
          sourceOwner,
          openOrder,
          sourceAssetKeys[i],
          sourceUSDCKey,
        );
      }
    }
  }

  // If nonexistent, create the source owner and signal provider associated addresses to receive the pooltokens
  let instructions: Array<TransactionInstruction> = [];
  let [
    targetPoolTokenKey,
    targetPTInstruction,
  ] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    sourceOwner.publicKey,
    poolMintKey,
    payer.publicKey,
  );
  targetPTInstruction ? instructions.push(targetPTInstruction) : null;

  let [
    sigProviderFeeReceiverKey,
    sigProvInstruction,
  ] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    poolHeader.signalProvider,
    poolMintKey,
    payer.publicKey,
  );
  sigProvInstruction ? instructions.push(sigProvInstruction) : null;

  let [
    bonfidaFeeReceiverKey,
    bonfidaFeeInstruction,
  ] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_FEE_KEY,
    poolMintKey,
    payer.publicKey,
  );
  bonfidaFeeInstruction ? instructions.push(bonfidaFeeInstruction) : null;

  let [
    bonfidaBuyAndBurnKey,
    bonfidaBNBInstruction,
  ] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_BNB_KEY,
    poolMintKey,
    payer.publicKey,
  );
  bonfidaBNBInstruction ? instructions.push(bonfidaBNBInstruction) : null;

  // @ts-ignore
  console.log(poolTokenAmount, new Numberu64(1000000 * poolTokenAmount));

  // Do the effective deposit
  console.log('Execute Buy in');
  let depositTxInstruction = depositInstruction(
    TOKEN_PROGRAM_ID,
    BONFIDABOT_PROGRAM_ID,
    sigProviderFeeReceiverKey,
    bonfidaFeeReceiverKey,
    bonfidaBuyAndBurnKey,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwner.publicKey,
    sourceAssetKeys,
    [poolSeed],
    // @ts-ignore
    new Numberu64(1000000 * poolTokenAmount),
  );
  instructions.push(depositTxInstruction);
  console.log(
    await signAndSendTransactionInstructions(
      connection,
      [sourceOwner],
      payer,
      instructions,
    ),
  );
}

/**
 * Returns the seeds of the pools managed by the given signal provider.
 * Returns all poolseeds for the current BonfidaBot program if no signal provider was given.
 * 
 * @param connection 
 * @param signalProviderKey 
 */
export async function getPoolsSeedsBySigProvider(
  connection: Connection,
  signalProviderKey?: PublicKey,
): Promise<Buffer[]> {
  const filter = [];
  // @ts-ignore
  const resp = await connection._rpcRequest('getProgramAccounts', [
    BONFIDABOT_PROGRAM_ID.toBase58(),
    {
      commitment: connection.commitment,
      filter,
      encoding: 'base64',
    },
  ]);
  if (resp.error) {
    throw new Error(resp.error.message);
  }
  let poolSeeds: Buffer[] = [];
  for (var account of resp.result) {
    let data = Buffer.from(account['account']['data'][0], 'base64');
    if (data.length < PoolHeader.LEN) {
      continue;
    }
    if (
      !signalProviderKey ||
      new PublicKey(data.slice(64, 96)).equals(signalProviderKey)
    ) {
      poolSeeds.push(data.slice(32, 64));
    }
  }
  return poolSeeds;
}

// Returns the pool token mint given a pool seed
export const getPoolTokenMintFromSeed = async (
  poolSeed: Buffer | Uint8Array,
) => {
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  return poolMintKey;
};

export const parseCreateOrderInstruction = (
  data: Buffer,
  poolInfo: PoolInfo,
  sig: ConfirmedSignatureInfo,
): PoolOrderInfo => {
  return {
    poolSeed: data.slice(1, 33),
    side: [OrderSide.Bid, OrderSide.Ask][data[33]],
    limitPrice: Numberu64.fromBuffer(data.slice(34, 42)).toNumber(),
    ratioOfPoolAssetsToTrade: Numberu16.fromBuffer(data.slice(42, 44)).toNumber(),
    orderType: [
      OrderType.Limit,
      OrderType.ImmediateOrCancel,
      OrderType.PostOnly,
    ][data[44]],
    clientOrderId: Numberu64.fromBuffer(data.slice(45, 53)).toNumber(),
    selfTradeBehavior: [
      SelfTradeBehavior.DecrementTake,
      SelfTradeBehavior.CancelProvide,
      SelfTradeBehavior.AbortTransaction,
    ][data[53]],
    market:
      poolInfo.authorizedMarkets[
        Numberu16.fromBuffer(data.slice(70, 72)).toNumber()
      ],
    transactionSignature: sig.signature,
    transactionSlot: sig.slot
  };
};

export const getPoolOrdersInfosFromSignature = async (
  connection: Connection,
  poolInfo: PoolInfo,
  sig: ConfirmedSignatureInfo,
): Promise<PoolOrderInfo[] | undefined> => {
  let t = await connection.getConfirmedTransaction(sig.signature);
  
  let x = t?.transaction.instructions.map(i => {
    if (i.programId.toBase58() === BONFIDABOT_PROGRAM_ID.toBase58() && i.data[0] == 3) {
      return parseCreateOrderInstruction(i.data, poolInfo, sig);
    }
  });
  return x?.filter(o => o) as PoolOrderInfo[] | undefined;
};

export const getPoolOrderInfos = async (
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
  n: number,
): Promise<PoolOrderInfo[]> => {
  // TODO: this will return less than n orders if the n orders aren't contained within the last 1000 pool transactions
  // TODO: this doesn't track what portion of the order is actually matched.
  let poolInfo = await fetchPoolInfo(connection, poolSeed);

  console.log(poolInfo.address.toBase58());

  let confirmedsignatures = await connection.getConfirmedSignaturesForAddress2(
    poolInfo.address,
  );

  console.log('Confirmed signatures retrieved: %s', confirmedsignatures.length);

  let infos = ((
    await Promise.all(
      confirmedsignatures.map(s =>
        getPoolOrdersInfosFromSignature(connection, poolInfo, s),
      ),
    )
  ).filter(o => o) as PoolOrderInfo[][]).flat();
  return infos.slice(0, n);
};
