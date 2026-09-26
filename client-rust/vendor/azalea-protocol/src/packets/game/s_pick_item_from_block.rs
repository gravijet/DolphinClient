use azalea_buf::AzBuf;
use azalea_core::position::BlockPos;
use azalea_protocol_macros::ServerboundGamePacket;

/// Real 26.1 wire format, decompiled from `ServerboundPickItemFromBlockPacket`
/// (a record of `(BlockPos pos, boolean includeData)`) — azalea's published
/// struct still carried an old `{slot: u32}` shape from before this packet
/// was reworked to let the server search the whole inventory rather than
/// just the hotbar.
#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundPickItemFromBlock {
    pub pos: BlockPos,
    pub include_data: bool,
}
