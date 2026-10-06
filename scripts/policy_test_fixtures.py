"""Canonical-shaped synthetic receipt fixtures; these are never proof evidence."""
def receipt(kind="EpochAggregated", index=0, event_count=16, lot_count=2, namespace=0):
    def word(value): return "0x" + f"{value:064x}"
    anchor = "0x" + f"{namespace + 2:040x}"
    epoch = str(index + 1)
    result = {"status": 1, "protocolVersion": 2, "verificationKind": "sp1-groth16",
              "transactionHash": word(100000 + namespace * 1000 + index), "gasUsed": "1000", "calldataBytes": 320,
              "registry": "0x" + "1" * 40, "anchor": anchor, "programVKey": word(9), "publicValuesBytes": 352,
              "policyValid": True, "epochId": epoch, "oldNullifierRoot": word(index + 1), "newNullifierRoot": word(index + 2),
              "certificateRoot": "0x" + "11" * 32, "polygonCommitment": "0x" + "22" * 32,
              "eventCount": event_count}
    args = {"epochId": epoch}
    if kind == "EpochAggregated":
        result.update(epochCommitment=word(20 + index), lotCount=lot_count, leafVKeyDigest="0x" + "44" * 32, aggregateVKeyDigest="0x" + "55" * 32)
        args.update(epochCommitment=result["epochCommitment"], oldNullifierRoot=result["oldNullifierRoot"],
                    newNullifierRoot=result["newNullifierRoot"], lotCount=str(lot_count), eventCount=str(event_count))
    elif kind in ("EpochAnchored", "TokenMinted"):
        result.update(eventBatchDigest=word(20 + index), nullifier=word(30 + index), role=4, threshold=900)
        args["eventBatchDigest"] = result["eventBatchDigest"]
        if kind == "EpochAnchored":
            args.update(oldNullifierRoot=result["oldNullifierRoot"], newNullifierRoot=result["newNullifierRoot"], nullifier=result["nullifier"])
        else:
            args.update(tokenId=word(100 + index), eventCount=str(event_count))
    elif kind == "hash":
        return {"status": 1, "protocolVersion": 2, "verificationKind": "hash-on-chain",
                "transactionHash": result["transactionHash"], "gasUsed": "1000", "calldataBytes": 320,
                "anchor":anchor,"shipmentId":epoch,"digest":word(20+index),"eventCount":event_count,
                "anchorEvent":{"name":"ShipmentAnchored","address":anchor,"transactionHash":result["transactionHash"],
                    "blockHash":word(200000+index),"args":{"shipmentId":epoch,"digest":word(20+index),"eventCount":str(event_count)}}}
    else:
        raise ValueError("unknown fixture event")
    result["anchorEvent"] = {"name": kind, "address": anchor, "transactionHash": result["transactionHash"],
                             "blockHash": word(200000 + index), "args": args}
    return result
