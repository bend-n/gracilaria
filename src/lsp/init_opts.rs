use Default::default;
use lsp_types::*;
use serde_json::json;
pub fn get(workspace: WorkspaceFolder) -> InitializeParams {
    InitializeParams {
        process_id: Some(std::process::id()),

        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..default()
            }),
            workspace: Some(WorkspaceClientCapabilities {
                symbol: Some(WorkspaceSymbolClientCapabilities {
                    symbol_kind: Some(SymbolKindCapability {
                        value_set: Some(SymbolKind::ALL.to_vec()),
                    }),
                    tag_support: Some(TagSupport {
                        value_set: SymbolTag::ALL.to_vec(),
                    }),
                    resolve_support: Some(
                        WorkspaceSymbolResolveSupportCapability {
                            properties: vec!["location".into()],
                        },
                    ),
                    ..default()
                }),
                diagnostic: Some(DiagnosticWorkspaceClientCapabilities {
                    refresh_support: Some(true),
                }),
                inlay_hint: Some(InlayHintWorkspaceClientCapabilities {
                    refresh_support: Some(true),
                }),
                workspace_edit: Some(WorkspaceEditClientCapabilities {
                    document_changes: Some(true),
                    resource_operations: Some(vec![
                        ResourceOperationKind::Create,
                        ResourceOperationKind::Rename,
                        ResourceOperationKind::Delete,
                    ]),
                    failure_handling: Some(FailureHandlingKind::Abort),
                    normalizes_line_endings: Some(false),
                    change_annotation_support: Some(
                        ChangeAnnotationWorkspaceEditClientCapabilities {
                            groups_on_label: Some(false),
                        },
                    ),
                }),
                ..default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                on_type_formatting: Some(
                    DocumentOnTypeFormattingClientCapabilities {
                        dynamic_registration: Some(false),
                    },
                ),
                document_highlight: Some(default()),
                formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                inlay_hint: Some(InlayHintClientCapabilities {
                    dynamic_registration: None,
                    resolve_support: Some(
                        InlayHintResolveClientCapabilities {
                            properties: vec![
                                "textEdits".into(),
                                "tooltip".into(),
                                "label.tooltip".into(),
                                "label.command".into(),
                            ],
                        },
                    ),
                }),
                document_symbol: Some(DocumentSymbolClientCapabilities {
                    tag_support: Some(TagSupport {
                        value_set: SymbolTag::ALL.to_vec(),
                    }),
                    symbol_kind: Some(SymbolKindCapability {
                        value_set: Some(SymbolKind::ALL.to_vec()),
                    }),
                    hierarchical_document_symbol_support: Some(true),
                    ..default()
                }),
                definition: Some(GotoCapability {
                    link_support: Some(true),
                    ..default()
                }),
                code_action: Some(CodeActionClientCapabilities {
                    data_support: Some(true),
                    resolve_support: Some(
                        CodeActionCapabilityResolveSupport {
                            properties: vec!["edit".to_string()],
                        },
                    ),
                    code_action_literal_support: Some(
                        CodeActionLiteralSupport {
                            code_action_kind:
                                CodeActionKindLiteralSupport {
                                    value_set: [
                                        "",
                                        "Empty",
                                        "QuickFix",
                                        "Refactor",
                                        "RefactorExtract",
                                        "RefactorInline",
                                        "RefactorRewrite",
                                        "Source",
                                        "SourceOrganizeImports",
                                        "quickfix",
                                        "refactor",
                                        "refactor.extract",
                                        "refactor.inline",
                                        "refactor.rewrite",
                                        "source",
                                        "source.organizeImports",
                                    ]
                                    .map(String::from)
                                    .into(),
                                },
                        },
                    ),
                    ..default()
                }),
                rename: Some(RenameClientCapabilities {
                    prepare_support: Some(true),
                    prepare_support_default_behavior: Some(
                        PrepareSupportDefaultBehavior::IDENTIFIER,
                    ),
                    honors_change_annotations: Some(false),
                    ..default()
                }),
                hover: Some(HoverClientCapabilities {
                    dynamic_registration: None,
                    content_format: Some(vec![
                        MarkupKind::PlainText,
                        MarkupKind::Markdown,
                    ]),
                }),
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: None,
                    related_document_support: Some(true),
                }),
                publish_diagnostics: Some(
                    PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        code_description_support: Some(true),
                        data_support: Some(true),
                        ..default()
                    },
                ),
                signature_help: Some(SignatureHelpClientCapabilities {
                    dynamic_registration: None,
                    signature_information: Some(
                        SignatureInformationSettings {
                            documentation_format: Some(vec![
                                MarkupKind::Markdown,
                                MarkupKind::PlainText,
                            ]),
                            parameter_information: Some(
                                ParameterInformationSettings {
                                    label_offset_support: Some(true),
                                },
                            ),
                            active_parameter_support: Some(true),
                        },
                    ),
                    context_support: Some(false),
                }),
                completion: Some(CompletionClientCapabilities {
                    dynamic_registration: Some(false),
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(true),
                        commit_characters_support: Some(true),
                        documentation_format: Some(vec![
                            MarkupKind::Markdown,
                            MarkupKind::PlainText,
                        ]),
                        deprecated_support: None,
                        preselect_support: None,
                        tag_support: Some(TagSupport {
                            value_set: vec![CompletionItemTag::DEPRECATED],
                        }),

                        resolve_support: Some(
                            CompletionItemCapabilityResolveSupport {
                                properties: vec![
                                    "additionalTextEdits".into(),
                                    "documentation".into(),
                                ],
                            },
                        ),
                        insert_replace_support: Some(false),
                        insert_text_mode_support: Some(
                            InsertTextModeSupport {
                                value_set: vec![InsertTextMode::AS_IS],
                            },
                        ),
                        label_details_support: Some(true),
                        ..default()
                    }),
                    completion_item_kind: Some(
                        CompletionItemKindCapability {
                            value_set: Some(vec![
                                CompletionItemKind::TEXT,
                                CompletionItemKind::METHOD,
                                CompletionItemKind::FUNCTION,
                                CompletionItemKind::CONSTRUCTOR,
                                CompletionItemKind::FIELD,
                                CompletionItemKind::VARIABLE,
                                CompletionItemKind::CLASS,
                                CompletionItemKind::INTERFACE,
                                CompletionItemKind::MODULE,
                                CompletionItemKind::PROPERTY,
                                CompletionItemKind::UNIT,
                                CompletionItemKind::VALUE,
                                CompletionItemKind::ENUM,
                                CompletionItemKind::KEYWORD,
                                CompletionItemKind::SNIPPET,
                                CompletionItemKind::COLOR,
                                CompletionItemKind::FILE,
                                CompletionItemKind::REFERENCE,
                                CompletionItemKind::FOLDER,
                                CompletionItemKind::ENUM_MEMBER,
                                CompletionItemKind::CONSTANT,
                                CompletionItemKind::STRUCT,
                                CompletionItemKind::EVENT,
                                CompletionItemKind::OPERATOR,
                                CompletionItemKind::TYPE_PARAMETER,
                            ]),
                            // value_set: Some(vec![CompletionItemKind::]),
                        },
                    ),
                    context_support: None,
                    insert_text_mode: Some(InsertTextMode::AS_IS),
                    completion_list: Some(CompletionListCapability {
                        item_defaults: None,
                    }),
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        range: Some(true),
                        full: Some(
                            lsp_types::SemanticTokensFullOptions::Bool(
                                true,
                            ),
                        ),
                    },
                    token_modifiers: [
                        "associated",
                        "attribute",
                        "callable",
                        "constant",
                        "consuming",
                        "controlFlow",
                        "crateRoot",
                        "injected",
                        "intraDocLink",
                        "library",
                        "macro",
                        "mutable",
                        "procMacro",
                        "public",
                        "reference",
                        "trait",
                        "unsafe",
                    ]
                    .map(|x| x.into())
                    .to_vec(),
                    overlapping_token_support: Some(true),
                    multiline_token_support: Some(true),
                    server_cancel_support: Some(false),
                    augments_syntax_tokens: Some(false),
                    ..default()
                }),
                ..default()
            }),

            general: Some(GeneralClientCapabilities {
                markdown: Some(MarkdownClientCapabilities {
                    version: Some("1.0.0".into()),
                    parser: "markdown".into(),
                    allowed_tags: Some(vec![]),
                }),
                position_encodings: Some(vec![PositionEncodingKind::UTF8]),
                ..default()
            }),
            experimental: Some(json! {{
                "matchingBrace": true,
                "snippetTextEdit": true,
                "colorDiagnosticOutput": true,
                "codeActionGroup": true,
                "serverStatusNotification": true,
                "hoverActions": true,
                "workspaceSymbolScopeKindFiltering": true,
                "onEnter": true,
                "localDocs": true,
            }}),
            ..default()
        },
        client_info: Some(ClientInfo {
            name: "gracilaria".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        initialization_options: Some(json! {{
            "cargo": {
                "buildScripts": { "enable": true }
            },
            "procMacro": {
                "enable": true,
                "attributes": { "enable": true }
            },
            "hover": {
                "documentation": {
                    "keywords": { "enable": false },
                },
            },
            "inlayHints": {
                "closureReturnTypeHints": { "enable": "with_block" },
                "closingBraceHints": { "minLines": 5 },
                "closureStyle": "rust_analyzer",
                "genericParameterHints": { "type": { "enable": true } },
                "rangeExclusiveHints": { "enable": true },
                "closureCaptureHints": { "enable": true },
            },
            "typing": { "triggerChars": ".=<>{(+" },
            "assist": { "preferSelf": true },
            "checkOnSave": true,
            "diagnostics": { "enable": true },
            "semanticHighlighting": {
                "punctuation": {
                    "separate": {
                        "macroBang": true
                    },
                    "specialization": { "enable": true },
                    "enable": true
                }
            },
            "workspace": {
                "symbol": {
                    "search": { "limit": 1024 }
                }
            },
            "showUnlinkedFileNotification": false,
            "completion": {
                "fullFunctionSignatures": { "enable": true, },
                "autoIter": { "enable": false, },
                "autoImport": { "enable": true, },
                "termSearch": { "enable": true, },
                "autoself": { "enable": true, },
                "privateEditable": { "enable": true },
            },
            "imports": {
                "granularity": "group",
            },
        }}),
        trace: None,
        workspace_folders: Some(vec![workspace]),

        ..default()
    }
}
