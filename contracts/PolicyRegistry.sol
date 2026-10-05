// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice The deployer approves or revokes exact certificate/polygon root pairs.
contract PolicyRegistry {
    address public immutable owner;
    mapping(bytes32 certificateRoot => mapping(bytes32 polygonCommitment => bool)) public allowedPolicies;

    error Unauthorized(address caller);
    event PolicyRootsSet(bytes32 indexed certificateRoot, bytes32 indexed polygonCommitment, bool allowed);

    constructor() { owner = msg.sender; }

    function setPolicyRoots(bytes32 certificateRoot, bytes32 polygonCommitment, bool allowed) external {
        if (msg.sender != owner) revert Unauthorized(msg.sender);
        allowedPolicies[certificateRoot][polygonCommitment] = allowed;
        emit PolicyRootsSet(certificateRoot, polygonCommitment, allowed);
    }
}
