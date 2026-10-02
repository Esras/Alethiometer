"""Tiny stand-in for bge-small-en-v1.5 with the same interface: inputs
input_ids / attention_mask / token_type_ids [1, S] int64, output
[1, S, 384] float. Every position gets the masked mean of the sequence added,
so the first (CLS) row depends on the text. Regenerate with `python3 make.py`
(needs the `onnx` package)."""
import json
import random

import onnx
from onnx import TensorProto, helper

DIM = 384
VOCAB = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "cutover", "plan", "dentist", "renewal", "datadog", "word", "the", "a"]
random.seed(7)
emb = [random.uniform(-1, 1) for _ in range(len(VOCAB) * DIM)]
typ = [random.uniform(-0.1, 0.1) for _ in range(2 * DIM)]

ids = helper.make_tensor_value_info("input_ids", TensorProto.INT64, [1, "S"])
mask = helper.make_tensor_value_info("attention_mask", TensorProto.INT64, [1, "S"])
types = helper.make_tensor_value_info("token_type_ids", TensorProto.INT64, [1, "S"])
out = helper.make_tensor_value_info("last_hidden_state", TensorProto.FLOAT, [1, "S", DIM])
nodes = [
    helper.make_node("Gather", ["E", "input_ids"], ["g"], axis=0),
    helper.make_node("Gather", ["T", "token_type_ids"], ["t"], axis=0),
    helper.make_node("Cast", ["attention_mask"], ["mf"], to=TensorProto.FLOAT),
    helper.make_node("Unsqueeze", ["mf", "ax"], ["m3"]),
    helper.make_node("Mul", ["g", "m3"], ["gm"]),
    helper.make_node("ReduceMean", ["gm", "ax1"], ["mean"], keepdims=1),
    helper.make_node("Add", ["gm", "mean"], ["s1"]),
    helper.make_node("Add", ["s1", "t"], ["last_hidden_state"]),
]
inits = [
    helper.make_tensor("E", TensorProto.FLOAT, [len(VOCAB), DIM], emb),
    helper.make_tensor("T", TensorProto.FLOAT, [2, DIM], typ),
    helper.make_tensor("ax", TensorProto.INT64, [1], [-1]),
    helper.make_tensor("ax1", TensorProto.INT64, [1], [1]),
]
graph = helper.make_graph(nodes, "tiny-bge", [ids, mask, types], [out], inits)
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 18)])
model.ir_version = 8
onnx.checker.check_model(model)
onnx.save(model, "model.onnx")

tok = {
    "version": "1.0",
    "truncation": None,
    "padding": None,
    "added_tokens": [{"id": i, "content": t, "single_word": False, "lstrip": False, "rstrip": False, "normalized": False, "special": True} for i, t in enumerate(VOCAB[:4])],
    "normalizer": {"type": "BertNormalizer", "clean_text": True, "handle_chinese_chars": True, "strip_accents": None, "lowercase": True},
    "pre_tokenizer": {"type": "BertPreTokenizer"},
    "post_processor": {
        "type": "TemplateProcessing",
        "single": [{"SpecialToken": {"id": "[CLS]", "type_id": 0}}, {"Sequence": {"id": "A", "type_id": 0}}, {"SpecialToken": {"id": "[SEP]", "type_id": 0}}],
        "pair": [{"SpecialToken": {"id": "[CLS]", "type_id": 0}}, {"Sequence": {"id": "A", "type_id": 0}}, {"SpecialToken": {"id": "[SEP]", "type_id": 0}}, {"Sequence": {"id": "B", "type_id": 1}}, {"SpecialToken": {"id": "[SEP]", "type_id": 1}}],
        "special_tokens": {"[CLS]": {"id": "[CLS]", "ids": [2], "tokens": ["[CLS]"]}, "[SEP]": {"id": "[SEP]", "ids": [3], "tokens": ["[SEP]"]}},
    },
    "decoder": {"type": "WordPiece", "prefix": "##", "cleanup": True},
    "model": {"type": "WordPiece", "unk_token": "[UNK]", "continuing_subword_prefix": "##", "max_input_chars_per_word": 100, "vocab": {t: i for i, t in enumerate(VOCAB)}},
}
with open("tokenizer.json", "w") as f:
    json.dump(tok, f, indent=1)
