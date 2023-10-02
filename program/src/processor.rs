use crate::error::AuctionError;
use crate::instruction::AuctionInstruction;
use crate::state::Auction;
use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program::{invoke, invoke_signed};
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account as TokenAccount;
use std::ops::Add;

pub struct Processor;

impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = AuctionInstruction::unpack(instruction_data)?;
        match instruction {
            AuctionInstruction::Exhibit {
                initial_price,
                seconds,
            } => {
                msg!("Initializing Auction...");
                Self::process_exhibit(accounts, initial_price, seconds, program_id)
            }
            AuctionInstruction::Bid { price } => {
                msg!("Placing a Bid in the Auction...");
                Self::process_bid(accounts, price, program_id)
            }
            AuctionInstruction::Cancel {} => {
                msg!("Cancelling the Auction ...");
                Self::process_cancel(accounts, program_id)
            }
            AuctionInstruction::Close {} => {
                msg!("Closing the Auction ...");
                Self::closing_the_process(accounts, program_id)
            }
        }
    }

    fn process_exhibit(
        accounts: &[AccountInfo],
        initial_price: u64,
        auction_duration_sec: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let accouint_of_exhibitor = next_account_info(account_info_iter)?;

        if !accouint_of_exhibitor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let exhibitor_nft_account = next_account_info(account_info_iter)?;
        let exhibitor_nft_temp_account = next_account_info(account_info_iter)?;
        let exhibitor_ft_receiving_account = next_account_info(account_info_iter)?;

        let escrow_account = next_account_info(account_info_iter)?;
        let sys_var_rent_account = next_account_info(account_info_iter)?;

        let rent = &Rent::from_account_info(sys_var_rent_account)?;
        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {
            return Err(AuctionError::NotRentExempt.into());
        }

        let mut auction_info = Auction::unpack_unchecked(&escrow_account.try_borrow_data()?)?;
        if auction_info.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        let sys_var_clock_account = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(sys_var_clock_account)?;

        auction_info.is_initialized = true;
        auction_info.exhibitor_pubkey = *accouint_of_exhibitor.key;
        auction_info.exhibiting_nft_temp_pubkey = *exhibitor_nft_temp_account.key;
        auction_info.exhibitor_ft_receiving_pubkey = *exhibitor_ft_receiving_account.key;
        auction_info.price = initial_price;
        auction_info.end_at = clock.unix_timestamp.add(auction_duration_sec as i64);
        Auction::pack(auction_info, &mut escrow_account.try_borrow_mut_data()?)?;

        let (pda, _bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);
        let program_of_token = next_account_info(account_info_iter)?;

        let exhibit_ix = spl_token::instruction::transfer(
            program_of_token.key,
            exhibitor_nft_account.key,
            exhibitor_nft_temp_account.key,
            accouint_of_exhibitor.key,
            &[], // authority_pubkey is default signer when the signer_pubkeys is empty.
            1,
        )?;
        msg!("Transferring the NFT to the Escrow Account...");
        invoke(
            &exhibit_ix,
            &[
                exhibitor_nft_account.clone(),
                exhibitor_nft_temp_account.clone(),
                accouint_of_exhibitor.clone(),
                program_of_token.clone(),
            ],
        )?;

        let owner_change_ix = spl_token::instruction::set_authority(
            program_of_token.key,
            exhibitor_nft_temp_account.key,
            Some(&pda),
            spl_token::instruction::AuthorityType::AccountOwner,
            accouint_of_exhibitor.key,
            &[], // owner_pubkey is default signer when the signer_pubkeys is empty.
        )?;
        msg!("Changing ownership of the token account...");
        invoke(
            &owner_change_ix,
            &[
                exhibitor_nft_temp_account.clone(),
                accouint_of_exhibitor.clone(),
                program_of_token.clone(),
            ],
        )?;
        Ok(())
    }

    fn process_bid(accounts: &[AccountInfo], price: u64, program_id: &Pubkey) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let bidder_account = next_account_info(account_info_iter)?;

        if !bidder_account.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        let highest_bidder_account = next_account_info(account_info_iter)?;
        let highest_bidder_ft_temp_account = next_account_info(account_info_iter)?;
        let highest_bidder_ft_returning_account = next_account_info(account_info_iter)?;

        let bidder_ft_temp_account = next_account_info(account_info_iter)?;
        let bidder_ft_account = next_account_info(account_info_iter)?;

        let escrow_account = next_account_info(account_info_iter)?;
        let mut auction_info = Auction::unpack(&escrow_account.try_borrow_data()?)?;

        let sys_var_clock_account = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(sys_var_clock_account)?;

        if auction_info.end_at <= clock.unix_timestamp {
            return Err(AuctionError::InactiveAuction.into());
        }

        if auction_info.price >= price {
            return Err(AuctionError::InsufficientBidPrice.into());
        }

        if auction_info.highest_bidder_ft_temp_pubkey != *highest_bidder_ft_temp_account.key {
            return Err(AuctionError::InvalidInstruction.into());
        }
        if auction_info.highest_bidder_ft_returning_pubkey
            != *highest_bidder_ft_returning_account.key
        {
            return Err(AuctionError::InvalidInstruction.into());
        }
        if auction_info.highest_bidder_pubkey != *highest_bidder_account.key {
            return Err(AuctionError::InvalidInstruction.into());
        }
        if auction_info.highest_bidder_pubkey == *bidder_account.key {
            return Err(AuctionError::AlreadyBid.into());
        }
        let program_of_token = next_account_info(account_info_iter)?;
        let pda_account = next_account_info(account_info_iter)?;
        let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);

        let transfer_to_escrow_ix = spl_token::instruction::transfer(
            program_of_token.key,
            bidder_ft_account.key,
            bidder_ft_temp_account.key,
            bidder_account.key,
            &[], 
            price,
        )?;
        msg!("Transferring FT to the Escrow Account from the bidder...");
        invoke(
            &transfer_to_escrow_ix,
            &[
                bidder_ft_account.clone(),
                bidder_ft_temp_account.clone(),
                bidder_account.clone(),
                program_of_token.clone(),
            ],
        )?;

        let owner_change_ix = spl_token::instruction::set_authority(
            program_of_token.key,
            bidder_ft_temp_account.key,
            Some(&pda),
            spl_token::instruction::AuthorityType::AccountOwner,
            bidder_account.key,
            &[], // owner_pubkey is default signer when the signer_pubkeys is empty.
        )?;
        msg!("Changing ownership of the token account...");
        invoke(
            &owner_change_ix,
            &[
                bidder_ft_temp_account.clone(),
                bidder_account.clone(),
                program_of_token.clone(),
            ],
        )?;

        if auction_info.highest_bidder_pubkey != Pubkey::default(){
            let transfer_to_previous_bidder_ix = spl_token::instruction::transfer(
                program_of_token.key,
                highest_bidder_ft_temp_account.key,
                highest_bidder_ft_returning_account.key,
                &pda,
                &[], // authority_pubkey is default signer when the signer_pubkeys is empty.
                auction_info.price,
            )?;
            msg!("Transferring FT to the previous highest bidder from the escrow account...");
            let signers_seeds: &[&[&[u8]]] = &[&[&b"escrow"[..], &[bump_seed]]];
            invoke_signed(
                &transfer_to_previous_bidder_ix,
                &[
                    highest_bidder_ft_temp_account.clone(),
                    highest_bidder_ft_returning_account.clone(),
                    pda_account.clone(),
                    program_of_token.clone(),
                ],
                signers_seeds,
            );

            Self::close_temporary_ft(
                program_of_token,
                highest_bidder_ft_temp_account,
                highest_bidder_account,
                pda,
                pda_account,
                signers_seeds,
            )?;
        }

        auction_info.price = price;
        auction_info.highest_bidder_pubkey = *bidder_account.key;
        auction_info.highest_bidder_ft_temp_pubkey = *bidder_ft_temp_account.key;
        auction_info.highest_bidder_ft_returning_pubkey = *bidder_ft_account.key;
        Auction::pack(auction_info, &mut escrow_account.try_borrow_mut_data()?)?;
        Ok(())
    }

    fn process_cancel(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let accouint_of_exhibitor = next_account_info(account_info_iter)?;

        if !accouint_of_exhibitor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let exhibiting_nft_temp_account = next_account_info(account_info_iter)?;
        let exhibiting_nft_returning_account = next_account_info(account_info_iter)?;
        let escrow_account = next_account_info(account_info_iter)?;
        let auction_info = Auction::unpack(&escrow_account.try_borrow_data()?)?;

        if auction_info.exhibitor_pubkey != *accouint_of_exhibitor.key {
            return Err(ProgramError::InvalidAccountData);
        }
        if auction_info.exhibiting_nft_temp_pubkey != *exhibiting_nft_temp_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        if auction_info.highest_bidder_pubkey != Pubkey::default() {
            return Err(AuctionError::AlreadyBid.into());
        }

        let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);
        let program_of_token = next_account_info(account_info_iter)?;
        let pda_account = next_account_info(account_info_iter)?;
        let signers_seeds: &[&[&[u8]]] = &[&[&b"escrow"[..], &[bump_seed]]];

        let exhibiting_nft_temp_account_data =
            TokenAccount::unpack(&exhibiting_nft_temp_account.try_borrow_data()?)?;
        let transfer_nft_to_exhibitor_ix = spl_token::instruction::transfer(
            program_of_token.key,
            exhibiting_nft_temp_account.key,
            exhibiting_nft_returning_account.key,
            &pda,
            &[], 
            exhibiting_nft_temp_account_data.amount,
        )?;
        msg!("Transferring NFT to the Exhibitor.....");
        invoke_signed(
            &transfer_nft_to_exhibitor_ix,
            &[
                exhibiting_nft_temp_account.clone(),
                exhibiting_nft_returning_account.clone(),
                pda_account.clone(),
                program_of_token.clone(),
            ],
            signers_seeds,
        )?;

        Self::escrow_is_closing(
            program_of_token,
            exhibiting_nft_temp_account,
            accouint_of_exhibitor,
            pda,
            pda_account,
            escrow_account,
            signers_seeds,
        )
    }

    fn closing_the_process(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {let account_info_iter = &mut accounts.iter();let highest_bidder_account = next_account_info(account_info_iter)?;

        if !highest_bidder_account.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let accouint_of_exhibitor = next_account_info(account_info_iter)?;let exhibiting_nft_temp_account = next_account_info(account_info_iter)?;
        let exhibitor_ft_receiving_account = next_account_info(account_info_iter)?;let highest_bidder_ft_temp_account = next_account_info(account_info_iter)?;
        let highest_bidder_nft_receiving_account = next_account_info(account_info_iter)?;let escrow_account = next_account_info(account_info_iter)?;let auction_info = Auction::unpack(&escrow_account.try_borrow_data()?)?;

        let sys_var_clock_account = next_account_info(account_info_iter)?;let clock = &Clock::from_account_info(sys_var_clock_account)?;if auction_info.end_at > clock.unix_timestamp {
            msg!(
                "Auction will end in {} seconds",
                (auction_info.end_at - clock.unix_timestamp)
            );
            return Err(AuctionError::ActiveAuction.into());
        }if auction_info.exhibitor_pubkey != *accouint_of_exhibitor.key {
            return Err(ProgramError::InvalidAccountData);
        }if auction_info.exhibiting_nft_temp_pubkey != *exhibiting_nft_temp_account.key {
            return Err(ProgramError::InvalidAccountData);
        }if auction_info.exhibitor_ft_receiving_pubkey != *exhibitor_ft_receiving_account.key {
            return Err(ProgramError::InvalidAccountData);
        }if auction_info.highest_bidder_ft_temp_pubkey != *highest_bidder_ft_temp_account.key {
            return Err(ProgramError::InvalidAccountData);
        }if auction_info.highest_bidder_pubkey != *highest_bidder_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);
        let program_of_token = next_account_info(account_info_iter)?;
        let pda_account = next_account_info(account_info_iter)?;
        let signers_seeds: &[&[&[u8]]] = &[&[&b"escrow"[..], &[bump_seed]]];

        let exhibiting_nft_temp_account_data =
            TokenAccount::unpack(&exhibiting_nft_temp_account.try_borrow_data()?)?;

        let highest_bidder_nft_transfer = spl_token::instruction::transfer(
            program_of_token.key,
            exhibiting_nft_temp_account.key,
            &highest_bidder_nft_receiving_account.key,
            &pda,
            &[], 
            exhibiting_nft_temp_account_data.amount,
        )?;
        msg!("Transferring NFT to the Highest Bidder...");
        invoke_signed(
            &highest_bidder_nft_transfer,
            &[
                exhibiting_nft_temp_account.clone(),
                highest_bidder_nft_receiving_account.clone(),
                pda_account.clone(),
                program_of_token.clone(),
            ],
            signers_seeds,
        )?;

        let temp_account_data_of_highest_Bidder =
            TokenAccount::unpack(&highest_bidder_ft_temp_account.try_borrow_data()?)?;
        let transfer_ft_to_exhibitor_ix = spl_token::instruction::transfer(
            program_of_token.key,
            highest_bidder_ft_temp_account.key,
            &exhibitor_ft_receiving_account.key,
            &pda,
            &[], 
            temp_account_data_of_highest_Bidder.amount,
        )?;
        msg!("Transferring FT to the Exhibitor...");
        invoke_signed(
            &transfer_ft_to_exhibitor_ix,
            &[
                highest_bidder_ft_temp_account.clone(),
                exhibitor_ft_receiving_account.clone(),
                pda_account.clone(),
                program_of_token.clone(),
            ],
            signers_seeds,
        )?;

        Self::close_temporary_ft(
            program_of_token,
            highest_bidder_ft_temp_account,
            highest_bidder_account,
            pda,
            pda_account,
            signers_seeds,
        )?;

        Self::escrow_is_closing(
            program_of_token,
            exhibiting_nft_temp_account,
            accouint_of_exhibitor,
            pda,
            pda_account,
            escrow_account,
            signers_seeds,
        )
    }

    fn escrow_is_closing<'a, 'b>(
        program_of_token: &'a AccountInfo<'b>,
        exhibiting_nft_temp_account: &'a AccountInfo<'b>,
        accouint_of_exhibitor: &'a AccountInfo<'b>,
        pda: Pubkey,
        pda_account: &'a AccountInfo<'b>,
        escrow_account: &'a AccountInfo<'b>,
        signers_seed: &[&[&[u8]]],
    ) -> ProgramResult {
        let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
            program_of_token.key,
            exhibiting_nft_temp_account.key,
            accouint_of_exhibitor.key,
            &pda,
            &[], // owner_pubkey is default signer when the signer_pubkeys is empty.
        )?;
        msg!("Closing the exhibitor's NFT temporary account s it was temperary...");
        invoke_signed(
            &close_pdas_temp_acc_ix,
            &[
                exhibiting_nft_temp_account.clone(),
                accouint_of_exhibitor.clone(),
                pda_account.clone(),
                program_of_token.clone(),
            ],
            signers_seed,
        );

        msg!("Closing the Escrow Account...");
        **accouint_of_exhibitor.try_borrow_mut_lamports()? = accouint_of_exhibitor
            .lamports()
            .checked_add(escrow_account.lamports())
            .ok_or(AuctionError::AmountOverflow)?;
        **escrow_account.try_borrow_mut_lamports()? = 0;
        *escrow_account.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    fn close_temporary_ft<'a, 'b>(
        program_of_token: &'a AccountInfo<'b>,
        highest_bidder_ft_temp_account: &'a AccountInfo<'b>,
        highest_bidder_account: &'a AccountInfo<'b>,
        pda: Pubkey,
        pda_account: &'a AccountInfo<'b>,
        signers_seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        let close_highest_bidder_ft_temp_acc_ix = spl_token::instruction::close_account(
            program_of_token.key,
            highest_bidder_ft_temp_account.key,
            highest_bidder_account.key,
            &pda,
            &[],
        )?;
        msg!("Closing the Highest Bidder's FT temporary account...");
        invoke_signed(
            &close_highest_bidder_ft_temp_acc_ix,
            &[
                highest_bidder_ft_temp_account.clone(),
                highest_bidder_account.clone(),
                pda_account.clone(),
                program_of_token.clone(),
            ],
            signers_seeds,
        );

        Ok(())
    }
}