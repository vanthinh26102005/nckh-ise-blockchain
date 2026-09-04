// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice Official SP1 verifier interface, pinned with the local SP1 v6.4 toolchain.
interface ISP1Verifier {
    function verifyProof(
        bytes32 programVKey,
        bytes calldata publicValues,
        bytes calldata proofBytes
    ) external view;
}

interface ISP1VerifierWithHash is ISP1Verifier {
    function VERIFIER_HASH() external pure returns (bytes32);
}
