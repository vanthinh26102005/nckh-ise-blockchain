// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice E3 hash-on-chain baseline: one canonical EPCIS shipment digest per tx.
contract ShipmentHashAnchor {
    mapping(uint64 => bytes32) public shipmentDigests;

    error DuplicateShipment(uint64 shipmentId);
    error EmptyShipment();

    event ShipmentAnchored(uint64 indexed shipmentId, bytes32 indexed digest, uint32 eventCount);

    function anchor(uint64 shipmentId, bytes32 digest, uint32 eventCount) external {
        if (eventCount == 0) revert EmptyShipment();
        if (shipmentDigests[shipmentId] != bytes32(0)) revert DuplicateShipment(shipmentId);
        shipmentDigests[shipmentId] = digest;
        emit ShipmentAnchored(shipmentId, digest, eventCount);
    }
}
