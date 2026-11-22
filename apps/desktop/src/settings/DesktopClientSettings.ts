import { ClientSettingsSchema, type ClientSettings } from "@t3tools/contracts";
import { writeFileStringAtomically } from "@t3tools/shared/atomicWrite";
import { fromLenientJson } from "@t3tools/shared/schemaJson";
import { resolveSymlinkTarget } from "@t3tools/shared/symlink";
import { lock } from "proper-lockfile";
import * as Context from "effect/Context";
import * as Deferred from "effect/Deferred";
import * as SecureRandom from "effect/crypto/SecureRandom";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Ref from "effect/Ref";

import * as DesktopEnvironment from "../app/DesktopEnvironment.ts";

const ClientSettingsJson = fromLenientJson(ClientSettingsSchema);
const SettingsRecord = Schema.Record(Schema.String, Schema.Unknown);
const decodeSettingsRecord = Schema.decodeUnknownEffect(SettingsRecord);
const encodeSettingsRecord = Schema.encodeEffect(Schema.fromJsonString(SettingsRecord));
const decodeClientSettingsDocument = Schema.decodeEffect(fromLenientJson(SettingsRecord));
const decodeClientSettingsValue = Schema.decodeUnknownEffect(ClientSettingsSchema);
const decodeClientSettingsJson = Effect.fnUntraced(function* (raw: string) {
  const document = yield* decodeClientSettingsDocument(raw);
  // Select the shape before validation so invalid legacy settings cannot become defaults.
  const wrapped = Object.hasOwn(document, "settings");
  const values = wrapped ? yield* decodeSettingsRecord(document.settings) : document;
  const settings = yield* decodeClientSettingsValue(values);
  return { document, values, settings, wrapped };
});
const encodeClientSettingsJson = Schema.encodeEffect(ClientSettingsJson);

export class DesktopClientSettingsReadError extends Schema.TaggedError<DesktopClientSettingsReadError>()(
  "DesktopClientSettingsReadError",
  {
    operation: Schema.Literals(["read-file", "decode-document"]),
    path: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Desktop client settings read failed during ${this.operation} at ${this.path}.`;
  }
}

const DesktopClientSettingsWriteOperation = Schema.Literals([
  "create-temporary-file-name",
  "lock-file",
  "resolve-symlink",
  "encode-document",
  "create-directory",
  "write-temporary-file",
  "replace-settings-file",
]);

export class DesktopClientSettingsWriteError extends Schema.TaggedError<DesktopClientSettingsWriteError>()(
  "DesktopClientSettingsWriteError",
  {
    operation: DesktopClientSettingsWriteOperation,
    path: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Desktop client settings write failed during ${this.operation} at ${this.path}.`;
  }
}

export class DesktopClientSettings extends Context.Service<
  DesktopClientSettings,
  {
    readonly get: Effect.Effect<Option.Option<ClientSettings>, DesktopClientSettingsReadError>;
    readonly set: (
      settings: ClientSettings,
    ) => Effect.Effect<void, DesktopClientSettingsWriteError>;
  }
>()("@t3tools/desktop/settings/DesktopClientSettings") {}

export const readClientSettings = (
  fileSystem: FileSystem.FileSystem,
  settingsPath: string,
): Effect.Effect<Option.Option<ClientSettings>, DesktopClientSettingsReadError> =>
  fileSystem.readFileString(settingsPath).pipe(
    Effect.asSome,
    Effect.catchTags({
      PlatformError: (cause) =>
        cause.reason._tag === "NotFound"
          ? Effect.succeed(Option.none<string>())
          : Effect.logWarning("Could not read desktop client settings.", cause).pipe(
              Effect.annotateLogs({ settingsPath }),
              Effect.andThen(
                Effect.fail(
                  new DesktopClientSettingsReadError({
                    operation: "read-file",
                    path: settingsPath,
                    cause,
                  }),
                ),
              ),
            ),
    }),
    Effect.flatMap(
      Option.match({
        onNone: () => Effect.succeed(Option.none<ClientSettings>()),
        onSome: (raw) =>
          decodeClientSettingsJson(raw).pipe(
            Effect.map(({ settings }) => settings),
            Effect.asSome,
            Effect.catchTags({
              SchemaError: (cause) =>
                Effect.logWarning("Could not decode desktop client settings.", cause).pipe(
                  Effect.annotateLogs({ settingsPath }),
                  Effect.andThen(
                    Effect.fail(
                      new DesktopClientSettingsReadError({
                        operation: "decode-document",
                        path: settingsPath,
                        cause,
                      }),
                    ),
                  ),
                ),
            }),
          ),
      }),
    ),
  );

function withSettingsWriteLock<A, E, R>(
  settingsPath: string,
  update: (target: string) => Effect.Effect<A, E, R>,
) {
  const failed = (operation: typeof DesktopClientSettingsWriteOperation.Type, cause: unknown) =>
    new DesktopClientSettingsWriteError({ operation, path: settingsPath, cause });
  return Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const target = yield* resolveSymlinkTarget(settingsPath).pipe(
      Effect.mapError((cause) => failed("resolve-symlink", cause)),
    );
    yield* fs
      .makeDirectory(path.dirname(target), { recursive: true })
      .pipe(Effect.mapError((cause) => failed("create-directory", cause)));
    const compromised = yield* Deferred.make<never, DesktopClientSettingsWriteError>();
    const services = yield* Effect.context<never>();
    return yield* Effect.acquireUseRelease(
      Effect.tryPromise({
        try: () =>
          lock(target, {
            realpath: false,
            retries: { retries: 30, factor: 1.3, minTimeout: 20, maxTimeout: 200 },
            onCompromised: (cause) =>
              Effect.runSyncWith(services)(Deferred.fail(compromised, failed("lock-file", cause))),
          }),
        catch: (cause) => failed("lock-file", cause),
      }),
      () => Effect.raceFirst(update(target), Deferred.await(compromised)),
      (release) =>
        Effect.tryPromise({
          try: () => release(),
          catch: (cause) => failed("lock-file", cause),
        }).pipe(Effect.ignore),
    );
  });
}

export const patchClientAppearance = Effect.fn("desktop.clientSettings.setTypography")(function (
  settingsPath: string,
  changes: Partial<
    Pick<
      ClientSettings,
      "fontSmoothing" | "fontFamilySans" | "fontFamilyCode" | "chatWidth" | "appearanceContrast"
    >
  >,
) {
  return withSettingsWriteLock(settingsPath, (target) =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const raw = yield* fs.readFileString(target).pipe(
        Effect.catchIf(
          (error) => error.reason._tag === "NotFound",
          () => Effect.succeed("{}"),
        ),
      );
      const { document, values, wrapped } = yield* decodeClientSettingsJson(raw);
      const updated = { ...values, ...changes };
      const settings = yield* decodeClientSettingsValue(updated);
      const encoded = yield* encodeSettingsRecord(
        wrapped ? { ...document, settings: updated } : updated,
      );
      yield* writeFileStringAtomically({ filePath: target, contents: encoded + "\n" });
      return settings;
    }),
  );
});

const writeClientSettings = Effect.fnUntraced(function* (input: {
  readonly fileSystem: FileSystem.FileSystem;
  readonly settingsPath: string;
  readonly settings: ClientSettings;
  readonly suffix: string;
}): Effect.fn.Return<void, DesktopClientSettingsWriteError> {
  const tempPath = `${input.settingsPath}.${process.pid}.${input.suffix}.tmp`;
  const encoded = yield* encodeClientSettingsJson(input.settings).pipe(
    Effect.mapError(
      (cause) =>
        new DesktopClientSettingsWriteError({
          operation: "encode-document",
          path: input.settingsPath,
          cause,
        }),
    ),
  );
  yield* Effect.gen(function* () {
    yield* input.fileSystem.writeFileString(tempPath, `${encoded}\n`).pipe(
      Effect.mapError(
        (cause) =>
          new DesktopClientSettingsWriteError({
            operation: "write-temporary-file",
            path: tempPath,
            cause,
          }),
      ),
    );
    yield* input.fileSystem.rename(tempPath, input.settingsPath).pipe(
      Effect.mapError(
        (cause) =>
          new DesktopClientSettingsWriteError({
            operation: "replace-settings-file",
            path: input.settingsPath,
            cause,
          }),
      ),
    );
  }).pipe(Effect.ensuring(input.fileSystem.remove(tempPath, { force: true }).pipe(Effect.ignore)));
});

export const make = Effect.gen(function* () {
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const random = yield* SecureRandom.SecureRandom;

  return DesktopClientSettings.of({
    get: readClientSettings(fileSystem, environment.clientSettingsPath).pipe(
      Effect.withSpan("desktop.clientSettings.get"),
    ),
    set: Effect.fn("desktop.clientSettings.set")(function (settings) {
      return random.uuidV4().pipe(
        Effect.map((uuid) => uuid.replace(/-/g, "")),
        Effect.flatMap((suffix) =>
          withSettingsWriteLock(environment.clientSettingsPath, (target) =>
            writeClientSettings({ fileSystem, settingsPath: target, settings, suffix }),
          ).pipe(
            Effect.provideService(FileSystem.FileSystem, fileSystem),
            Effect.provideService(Path.Path, path),
          ),
        ),
      );
    }),
  });
});

export const layer = Layer.effect(DesktopClientSettings, make);

export const layerTest = (initialSettings: Option.Option<ClientSettings> = Option.none()) =>
  Layer.effect(
    DesktopClientSettings,
    Effect.gen(function* () {
      const settingsRef = yield* Ref.make(initialSettings);
      return DesktopClientSettings.of({
        get: Ref.get(settingsRef),
        set: Effect.fn("DesktopClientSettings.set")(function (settings) {
          return Ref.set(settingsRef, Option.some(settings));
        }),
      });
    }),
  );
