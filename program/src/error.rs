use solana_program::msg;
use thiserror::Error;

use solana_program::program_error::ProgramError;

#[derive(Error, Debug, Copy, Clone)]
pub enum AuctionError {#[error("Invalid Instruction Error: The provided instruction is not recognized.")]
    InvalidInstruction,#[error("Rent Exemption Error: The escrow account is not exempted from rent.")]
    NotRentExempt,#[error("Expected Amount Error: The expected amount is different from the actual value.")]
    ExpectedAmountMismatch,#[error("Amount Overflow Error: The operation would result in an amount overflow.")]
    AmountOverflow,#[error("Insufficient Bid Price Error: The bid amount is too low. Please increase your bid.")]
    InsufficientBidPrice,#[error("Bid Error: A bid has already been placed in this auction.")]
    AlreadyBid, #[error("Auction Inactive Error: The auction has concluded and is no longer active.")]
    InactiveAuction,#[error("Auction Active Error: The auction is still ongoing.")]
    ActiveAuction,#[error("No Bidders Error: There are no bidders participating in this auction.")]
    NoBidderFound,
}

impl From<AuctionError> for ProgramError {
    fn from(e: AuctionError) -> Self {
        msg!("Error: {:?}", e);
        ProgramError::Custom(e as u32)
    }
}