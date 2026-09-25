// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISP1Verifier} from "./sp1/ISP1Verifier.sol";

/// @notice E3 adapted token baseline. Uses this artifact's SP1 policy proof;
///         it does not reproduce VeCroToken's original cross-chain protocol.
contract VeCroTokenAdaptedAnchor {
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

    ISP1Verifier public immutable verifier;
    bytes32 public immutable policyProgramVKey;
    bytes32 public nullifierRoot;
    mapping(bytes32 => bool) public mintedTokens;

    error InvalidPolicyResult();
    error StateRootMismatch(bytes32 expected, bytes32 received);
    error DuplicateToken(bytes32 tokenId);

    event TokenMinted(bytes32 indexed tokenId, uint64 indexed epochId, bytes32 eventBatchDigest, uint8 eventCount);

    constructor(address verifier_, bytes32 policyProgramVKey_, bytes32 initialRoot_) {
        verifier = ISP1Verifier(verifier_);
        policyProgramVKey = policyProgramVKey_;
        nullifierRoot = initialRoot_;
    }

    function verifyAndMint(bytes calldata publicValues, bytes calldata proof) external {
        verifier.verifyProof(policyProgramVKey, publicValues, proof);
        PolicyPublicValues memory values = abi.decode(publicValues, (PolicyPublicValues));
        if (!values.valid || values.eventCount < 8 || values.eventCount > 64) revert InvalidPolicyResult();
        if (values.oldNullifierRoot != nullifierRoot) {
            revert StateRootMismatch(nullifierRoot, values.oldNullifierRoot);
        }
        bytes32 tokenId = keccak256(abi.encode(values.epochId, values.eventBatchDigest));
        if (mintedTokens[tokenId]) revert DuplicateToken(tokenId);
        mintedTokens[tokenId] = true;
        nullifierRoot = values.newNullifierRoot;
        emit TokenMinted(tokenId, values.epochId, values.eventBatchDigest, values.eventCount);
    }
}
