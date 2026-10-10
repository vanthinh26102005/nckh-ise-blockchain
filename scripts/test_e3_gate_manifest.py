#!/usr/bin/env python3
"""Gate metadata checks use synthetic bytes only; they never verify a proof."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("e3_gate_manifest.py")

def root(value):
    return "0x" + value * 64

def examples():
    fields = [1,root("1"),root("2"),root("3"),2,16,1,root("b"),root("c"),root("4"),root("5")]
    public_values = "0x" + "".join(value[2:] if isinstance(value,str) else f"{value:064x}" for value in fields)
    fixture = {"protocolVersion":2,"verificationKind":"sp1-groth16","sourceRevision":"1"*40,"vkey":root("a"),"leafVKeyDigest":root("b"),
        "aggregateVKeyDigest":root("c"),"publicValues":public_values,"proof":"0x4388a21c"+"0"*704,"epochId":"1",
        "oldNullifierRoot":root("1"),"newNullifierRoot":root("2"),"certificateRoot":root("4"),"polygonCommitment":root("5"),
        "fabricEventsAcknowledged":16,"fabricEventsReadback":16,"fabricEvidenceReused":False,
        "tamperedChildRejected":True,"leafCompressedProofMs":1,"aggregateGroth16ProofMs":1}
    receipt = {"protocolVersion":2,"verificationKind":"sp1-groth16","sourceRevision":"1"*40,"publicValuesBytes":352,"policyValid":True,
        "tamperedAggregateProofRejected":True,"tamperedAggregateValuesRejected":True,"programVKey":root("a"),"status":1,"epochId":"1","oldNullifierRoot":root("1"),"newNullifierRoot":root("2"),"certificateRoot":root("4"),
        "polygonCommitment":root("5"),"leafVKeyDigest":root("b"),"aggregateVKeyDigest":root("c"),"lotCount":"2","eventCount":"16",
        "transactionHash":root("d"),"anchor":"0x"+"e"*40,"registry":"0x"+"f"*40,"gasUsed":"123","calldataBytes":900,
        "anchorEvent":{"name":"EpochAggregated","address":"0x"+"e"*40,"transactionHash":root("d"),"blockHash":root("9"),
            "args":{"epochId":"1","epochCommitment":root("3"),"oldNullifierRoot":root("1"),"newNullifierRoot":root("2"),"lotCount":"2","eventCount":"16"}}}
    return fixture,receipt

class GateChecks(unittest.TestCase):
    def run_gate(self,fixture,receipt):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            for name,data in [("fixture",fixture),("receipt",receipt)]:
                (directory/f"{name}.json").write_text(json.dumps(data))
            result=subprocess.run([sys.executable,str(SCRIPT),"--fixture",str(directory/"fixture.json"),"--receipt",str(directory/"receipt.json"),
                "--out",str(directory/"gate.json"),"--git-commit","1"*40],capture_output=True,text=True)
            gate=json.loads((directory/"gate.json").read_text()) if (directory/"gate.json").exists() else None
            return result,gate

    def test_stub_receipt_cannot_pass_formal_gate(self):
        fixture,receipt=examples();receipt["verificationKind"]="contract-stub-test"
        result,gate=self.run_gate(fixture,receipt)
        self.assertNotEqual(result.returncode,0,"contract stub was accepted as formal evidence")
        self.assertIsNone(gate)

    def test_output_never_overwrites_an_existing_manifest(self):
        fixture,receipt=examples()
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            (directory/"fixture.json").write_text(json.dumps(fixture))
            (directory/"receipt.json").write_text(json.dumps(receipt))
            output=directory/"gate.json";output.write_text('{"preserved":true}\n')
            result=subprocess.run([sys.executable,str(SCRIPT),"--fixture",str(directory/"fixture.json"),"--receipt",str(directory/"receipt.json"),
                "--out",str(output),"--git-commit","1"*40],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertEqual(output.read_text(),'{"preserved":true}\n')

    def test_metadata_and_event_binding(self):
        fixture,receipt=examples()
        result,gate=self.run_gate(fixture,receipt)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(gate["protocolVersion"],2)
        self.assertEqual(gate["verificationKind"],"sp1-groth16")
        for section,field,value in [("fixture","sourceRevision","2"*40),("receipt","sourceRevision","2"*40),("receipt","tamperedAggregateProofRejected",False),("receipt","tamperedAggregateValuesRejected",False),("fixture","protocolVersion",1),("fixture","verificationKind","contract-stub-test"),
            ("fixture","fabricEventsReadback",15),("fixture","fabricEvidenceReused",True),
            ("fixture","publicValues",fixture["publicValues"][:-64]),("fixture","proof","0x"),("receipt","status",0),("receipt","programVKey",root("f")),
            ("receipt","epochId","2"),("receipt","certificateRoot",root("f")),("receipt","polygonCommitment",root("f")),
            ("receipt","leafVKeyDigest",root("f")),("receipt","aggregateVKeyDigest",root("f")),("receipt","oldNullifierRoot",root("f"))]:
            f,r=copy.deepcopy(fixture),copy.deepcopy(receipt)
            (f if section=="fixture" else r)[field]=value
            with self.subTest(section=section,field=field):
                result,gate=self.run_gate(f,r);self.assertNotEqual(result.returncode,0);self.assertIsNone(gate)
        for field,value in [("epochId","2"),("epochCommitment",root("f")),("eventCount","17"),("newNullifierRoot",root("f"))]:
            r=copy.deepcopy(receipt);r["anchorEvent"]["args"][field]=value
            with self.subTest(eventfield=field):
                result,gate=self.run_gate(fixture,r);self.assertNotEqual(result.returncode,0);self.assertIsNone(gate)
        for field,value in [("address","0x"+"a"*40),("transactionHash",root("f")),("name","TokenMinted")]:
            r=copy.deepcopy(receipt);r["anchorEvent"][field]=value
            with self.subTest(eventfield=field):
                result,gate=self.run_gate(fixture,r);self.assertNotEqual(result.returncode,0);self.assertIsNone(gate)

if __name__=="__main__":
    unittest.main()
