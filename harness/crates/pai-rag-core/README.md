# pai-rag-core

Native, deterministic core of the document RAG pipeline. It preserves the earlier store's
SQLite/FTS schema, section-aware chunking, embedding input, and reciprocal-rank fusion so an
existing library can be opened without rebuilding it.

`pai-rag` uses this crate in-process. Text, Markdown, code, structured text, HTML, DOCX,
and PDFs with a usable text layer are read natively. HTTP embedding, Qdrant, and optional
HTTP reranking are native as well.
