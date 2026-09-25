// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice E3 adaptation: per-shipment token ownership and digest anchoring.
/// @dev This is not a reproduction of VeCroToken's ABE/cross-chain protocol.
contract VeCroTokenAdapted {
    struct ShipmentToken {
        address owner;
        bytes32 digest;
    }

    error DuplicateShipment(uint64 shipmentId);
    error NotOwner(uint64 shipmentId);
    error InvalidRecipient();
    mapping(uint64 => ShipmentToken) public tokens;
    mapping(uint64 => bool) public exists;
    event ShipmentMinted(uint64 indexed shipmentId, address indexed owner, bytes32 digest);
    event ShipmentTransferred(uint64 indexed shipmentId, address indexed from, address indexed to);

    function mint(uint64 shipmentId, bytes32 digest) external {
        if (exists[shipmentId]) revert DuplicateShipment(shipmentId);
        exists[shipmentId] = true;
        tokens[shipmentId] = ShipmentToken(msg.sender, digest);
        emit ShipmentMinted(shipmentId, msg.sender, digest);
    }

    function transfer(uint64 shipmentId, address to) external {
        if (to == address(0)) revert InvalidRecipient();
        ShipmentToken storage token = tokens[shipmentId];
        if (token.owner != msg.sender) revert NotOwner(shipmentId);
        token.owner = to;
        emit ShipmentTransferred(shipmentId, msg.sender, to);
    }
}
