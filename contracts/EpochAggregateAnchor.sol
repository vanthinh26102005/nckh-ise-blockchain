// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISP1Verifier} from "./sp1/ISP1Verifier.sol";

/// @notice One SP1 aggregate Groth16 proof advances the nullifier root once per epoch.
contract EpochAggregateAnchor {
    struct EpochPublicValues {
        uint64 epochId;
        bytes32 oldNullifierRoot;
        bytes32 newNullifierRoot;
        bytes32 epochCommitment;
        uint64 lotCount;
        uint64 eventCount;
        bool valid;
        bytes32 leafVKeyDigest;
        bytes32 aggregateVKeyDigest;
    }

    error DuplicateEpoch(uint64 epochId);
    error InvalidEpochResult();
    error StateRootMismatch(bytes32 expected, bytes32 received);
    error UnexpectedEpoch(uint64 expected, uint64 received);
    error LeafVKeyMismatch(bytes32 expected, bytes32 received);

    ISP1Verifier public immutable verifier;
    bytes32 public immutable programVKey;
    bytes32 public immutable expectedLeafVKeyDigest;
    bytes32 public immutable expectedAggregateVKeyDigest;
    bytes32 public nullifierRoot;
    mapping(uint64 => bool) public anchoredEpochs;

    event EpochAggregated(
        uint64 indexed epochId,
        bytes32 indexed epochCommitment,
        bytes32 oldNullifierRoot,
        bytes32 newNullifierRoot,
        uint64 lotCount,
        uint64 eventCount
    );

    constructor(address verifier_, bytes32 programVKey_, bytes32 leafVKeyDigest_,
        bytes32 aggregateVKeyDigest_, bytes32 initialRoot_) {
        verifier = ISP1Verifier(verifier_);
        programVKey = programVKey_;
        expectedLeafVKeyDigest = leafVKeyDigest_;
        expectedAggregateVKeyDigest = aggregateVKeyDigest_;
        nullifierRoot = initialRoot_;
    }

    function verifyAndAnchor(
        uint64 expectedEpochId,
        bytes32 expectedOldNullifierRoot,
        bytes calldata publicValues,
        bytes calldata proof
    ) external {
        if (anchoredEpochs[expectedEpochId]) revert DuplicateEpoch(expectedEpochId);
        verifier.verifyProof(programVKey, publicValues, proof);
        EpochPublicValues memory values = abi.decode(publicValues, (EpochPublicValues));
        if (!values.valid || values.lotCount == 0 ||
            values.eventCount < values.lotCount * 8 || values.eventCount > values.lotCount * 64) {
            revert InvalidEpochResult();
        }
        if (values.epochId != expectedEpochId) revert UnexpectedEpoch(expectedEpochId, values.epochId);
        if (values.leafVKeyDigest != expectedLeafVKeyDigest) {
            revert LeafVKeyMismatch(expectedLeafVKeyDigest, values.leafVKeyDigest);
        }
        if (values.aggregateVKeyDigest != expectedAggregateVKeyDigest) {
            revert LeafVKeyMismatch(expectedAggregateVKeyDigest, values.aggregateVKeyDigest);
        }
        if (values.oldNullifierRoot != expectedOldNullifierRoot) {
            revert StateRootMismatch(expectedOldNullifierRoot, values.oldNullifierRoot);
        }
        if (values.oldNullifierRoot != nullifierRoot) {
            revert StateRootMismatch(nullifierRoot, values.oldNullifierRoot);
        }
        anchoredEpochs[values.epochId] = true;
        nullifierRoot = values.newNullifierRoot;
        emit EpochAggregated(values.epochId, values.epochCommitment, values.oldNullifierRoot,
            values.newNullifierRoot, values.lotCount, values.eventCount);
    }
}
