// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISP1Verifier} from "./sp1/ISP1Verifier.sol";
import {PolicyRegistry} from "./PolicyRegistry.sol";

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

    error InvalidPublicValuesLength(uint256 received);
    error PolicyRootsNotAllowed(bytes32 certificateRoot, bytes32 polygonCommitment);
    error StateRootNotAdvanced();

    PolicyRegistry public immutable policyRegistry;
    ISP1Verifier public immutable verifier;
    bytes32 public immutable programVKey;
    bytes32 public nullifierRoot;
    mapping(uint64 epochId => bytes32 eventBatchDigest) public anchoredEpochs;
    mapping(uint64 => bool) private epochAnchored;

    event EpochAnchored(
        uint64 indexed epochId,
        bytes32 indexed eventBatchDigest,
        bytes32 oldNullifierRoot,
        bytes32 newNullifierRoot,
        bytes32 nullifier
    );

    constructor(address verifier_, bytes32 programVKey_, bytes32 initialNullifierRoot_, address policyRegistry_) {
        policyRegistry = PolicyRegistry(policyRegistry_);
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
        if (epochAnchored[expectedEpochId]) revert DuplicateEpoch(expectedEpochId);
        if (publicValues.length != 352) revert InvalidPublicValuesLength(publicValues.length);
        verifier.verifyProof(programVKey, publicValues, proof);
        PolicyPublicValues memory values = abi.decode(publicValues, (PolicyPublicValues));
        if (!values.valid || values.role != 4 || values.threshold != 900 ||
            values.eventCount < 8 || values.eventCount > 64) revert InvalidPolicyResult();
        if (!policyRegistry.allowedPolicies(values.certificateRoot, values.polygonCommitment)) {
            revert PolicyRootsNotAllowed(values.certificateRoot, values.polygonCommitment);
        }
        if (values.newNullifierRoot == values.oldNullifierRoot) revert StateRootNotAdvanced();
        if (values.epochId != expectedEpochId) {
            revert UnexpectedEpoch(expectedEpochId, values.epochId);
        }
        if (values.oldNullifierRoot != expectedOldNullifierRoot) {
            revert StateRootMismatch(expectedOldNullifierRoot, values.oldNullifierRoot);
        }
        if (values.oldNullifierRoot != nullifierRoot) {
            revert StateRootMismatch(nullifierRoot, values.oldNullifierRoot);
        }
        epochAnchored[values.epochId] = true;
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
