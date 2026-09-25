// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice E3 hash-on-chain baseline: one canonical shipment digest per tx.
contract HashShipmentAnchor {
    error DuplicateShipment(uint64 shipmentId);
    mapping(uint64 => bytes32) public shipmentDigests;
    mapping(uint64 => bool) public exists;
    event ShipmentAnchored(uint64 indexed shipmentId, bytes32 digest);

    function anchor(uint64 shipmentId, bytes32 digest) external {
        if (exists[shipmentId]) revert DuplicateShipment(shipmentId);
        exists[shipmentId] = true;
        shipmentDigests[shipmentId] = digest;
        emit ShipmentAnchored(shipmentId, digest);
    }
}
