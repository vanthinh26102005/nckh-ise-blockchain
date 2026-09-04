// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISP1Verifier} from "./sp1/ISP1Verifier.sol";

/// @notice Anchors one SP1-verified EUDR policy transition per epoch.
contract EpochAnchor {
    struct PolicyPublicValues {
        uint64 epochId;
        bytes32 oldNullifierRoot;
        bytes32 newNullifierRoot;
        bytes32 eventBatchDigest;
        bytes32 certificateRoot;
        bytes32 polygonCommitment;
        bytes32 nullifier;
        uint8 role;
        uint32 threshold;
        uint8 eventCount;
        bool valid;
    }

    error DuplicateEpoch(uint64 epochId);
    error InvalidPolicyResult();
    error StateRootMismatch(bytes32 expected, bytes32 received);
    error UnexpectedEpoch(uint64 expected, uint64 received);

    ISP1Verifier public immutable verifier;
    bytes32 public immutable programVKey;
    bytes32 public nullifierRoot;
    mapping(uint64 epochId => bytes32 eventBatchDigest) public anchoredEpochs;

    event EpochAnchored(
        uint64 indexed epochId,
        bytes32 indexed eventBatchDigest,
        bytes32 oldNullifierRoot,
        bytes32 newNullifierRoot,
        bytes32 nullifier
    );

    constructor(address verifier_, bytes32 programVKey_, bytes32 initialNullifierRoot_) {
        verifier = ISP1Verifier(verifier_);
        programVKey = programVKey_;
        nullifierRoot = initialNullifierRoot_;
    }

    function verifyAndAnchor(
        uint64 expectedEpochId,
        bytes32 expectedOldNullifierRoot,
        bytes calldata publicValues,
        bytes calldata proof
    ) external {
        verifier.verifyProof(programVKey, publicValues, proof);
        PolicyPublicValues memory values = abi.decode(publicValues, (PolicyPublicValues));
        if (!values.valid) revert InvalidPolicyResult();
        if (values.epochId != expectedEpochId) {
            revert UnexpectedEpoch(expectedEpochId, values.epochId);
        }
        if (values.oldNullifierRoot != expectedOldNullifierRoot) {
            revert StateRootMismatch(expectedOldNullifierRoot, values.oldNullifierRoot);
        }
        if (values.oldNullifierRoot != nullifierRoot) {
            revert StateRootMismatch(nullifierRoot, values.oldNullifierRoot);
        }
        if (anchoredEpochs[values.epochId] != bytes32(0)) {
            revert DuplicateEpoch(values.epochId);
        }
        anchoredEpochs[values.epochId] = values.eventBatchDigest;
        nullifierRoot = values.newNullifierRoot;
        emit EpochAnchored(
            values.epochId,
            values.eventBatchDigest,
            values.oldNullifierRoot,
            values.newNullifierRoot,
            values.nullifier
        );
    }
}
