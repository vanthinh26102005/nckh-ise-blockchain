// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISP1Verifier} from "./sp1/ISP1Verifier.sol";

/// @notice Anchors a recursively verified E3 epoch proof once per epoch.
contract EpochAggregateAnchor {
    struct AggregatePublicValues {
        uint64 epochId;
        bytes32 oldNullifierRoot;
        bytes32 newNullifierRoot;
        bytes32 epochCommitment;
        uint32 lotCount;
        uint32 eventCount;
        bool valid;
    }

    error DuplicateEpoch(uint64 epochId);
    error InvalidAggregate();
    error StateRootMismatch(bytes32 expected, bytes32 received);
    error UnexpectedEpoch(uint64 expected, uint64 received);

    ISP1Verifier public immutable verifier;
    bytes32 public immutable programVKey;
    bytes32 public nullifierRoot;
    mapping(uint64 => bool) public anchoredEpochs;
    mapping(uint64 => bytes32) public epochCommitments;

    event EpochAggregated(
        uint64 indexed epochId,
        bytes32 indexed epochCommitment,
        bytes32 oldNullifierRoot,
        bytes32 newNullifierRoot,
        uint32 lotCount,
        uint32 eventCount
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
        AggregatePublicValues memory values = abi.decode(publicValues, (AggregatePublicValues));
        if (!values.valid || values.lotCount == 0 || values.eventCount == 0) {
            revert InvalidAggregate();
        }
        if (values.epochId != expectedEpochId) {
            revert UnexpectedEpoch(expectedEpochId, values.epochId);
        }
        if (values.oldNullifierRoot != expectedOldNullifierRoot) {
            revert StateRootMismatch(expectedOldNullifierRoot, values.oldNullifierRoot);
        }
        if (values.oldNullifierRoot != nullifierRoot) {
            revert StateRootMismatch(nullifierRoot, values.oldNullifierRoot);
        }
        if (anchoredEpochs[values.epochId]) revert DuplicateEpoch(values.epochId);
        anchoredEpochs[values.epochId] = true;
        epochCommitments[values.epochId] = values.epochCommitment;
        nullifierRoot = values.newNullifierRoot;
        emit EpochAggregated(
            values.epochId,
            values.epochCommitment,
            values.oldNullifierRoot,
            values.newNullifierRoot,
            values.lotCount,
            values.eventCount
        );
    }
}
