import { Compass, ExternalLink, Search } from "lucide-react";
import { useEffect, useState } from "react";
import {
  api,
  type ModelCard,
  type ModelLibrary,
  type ModelListing,
  type SavedModel,
} from "../api";
import { Button, Input, Select } from "../ui";

const THINGIVERSE_DEVELOPERS = "https://www.thingiverse.com/developers";

export interface ModelsDiscoverProps {
  library: ModelLibrary;
  onLibrary: (library: ModelLibrary) => void;
  query: string;
  onQuery: (query: string) => void;
  sort: string;
  onSort: (sort: string) => void;
  onSearch: () => void;
  searching: boolean;
  error: string | null;
  hits: ModelCard[];
  total: number;
  page: number;
  onPage: (page: number) => void;
  listing: ModelListing | null;
  listingId: string | null;
  onSelect: (id: string) => void;
  profileId: string | null;
  onProfile: (id: string) => void;
  fileId: string | null;
  onFile: (id: string) => void;
  onOpen: () => void;
  opening: boolean;
  link: string;
  onLink: (link: string) => void;
  onOpenLink: () => void;
  tokenConfigured: boolean;
  tokenDraft: string;
  onTokenDraft: (token: string) => void;
  onSaveToken: () => void;
  onClearToken: () => void;
  onOpenAccounts: () => void;
  onOpenListing: (url: string) => void;
}

function messageOf(err: unknown): string {
  if (typeof err === "string" && err.trim()) return err;
  if (err instanceof Error && err.message.trim()) return err.message;
  return "The model library could not complete that.";
}

function hue(name: string): number {
  let hash = 0;
  for (let index = 0; index < name.length; index += 1) hash = (hash * 33 + name.charCodeAt(index)) % 360;
  return hash;
}

function sortsFor(library: ModelLibrary): { value: string; label: string }[] {
  if (library === "thingiverse") {
    return [
      { value: "relevant", label: "Relevant" },
      { value: "popular", label: "Popular" },
      { value: "newest", label: "Newest" },
    ];
  }
  return [
    { value: "relevant", label: "Relevant" },
    { value: "popular", label: "Popular" },
    { value: "downloads", label: "Downloads" },
  ];
}

export function ModelsDiscover(props: ModelsDiscoverProps) {
  const thingiverseLocked = props.library === "thingiverse" && !props.tokenConfigured;
  const pageSize = 20;
  const pages = Math.max(1, Math.ceil(props.total / pageSize));
  const selected = props.listing && props.listing.card.id === props.listingId ? props.listing : null;

  return (
    <div className="models-view">
      <header className="models-intro">
        <p className="models-kicker">
          <Compass size={14} strokeWidth={1.5} aria-hidden />
          Discover
        </p>
        <h1>Models</h1>
        <p>Search MakerWorld and Thingiverse, then open a model in a new project. The creator and the licence stay with the file.</p>
      </header>

      <div className="models-sources" role="tablist" aria-label="Model library">
        <button
          type="button"
          role="tab"
          aria-selected={props.library === "makerworld"}
          className={props.library === "makerworld" ? "models-source is-on" : "models-source"}
          onClick={() => props.onLibrary("makerworld")}
        >
          MakerWorld
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={props.library === "thingiverse"}
          className={props.library === "thingiverse" ? "models-source is-on" : "models-source"}
          onClick={() => props.onLibrary("thingiverse")}
        >
          Thingiverse
        </button>
      </div>

      {thingiverseLocked ? (
        <section className="models-empty" aria-label="Thingiverse token">
          <h2>Thingiverse needs an app token</h2>
          <p>
            Create a desktop app on the{" "}
            <a href={THINGIVERSE_DEVELOPERS} target="_blank" rel="noreferrer">
              Thingiverse developer page
            </a>{" "}
            and paste the app token here. Shale does not ship a token, and it is not written into the project.
          </p>
          <label className="ui-field">
            <span className="ui-field__label">App token</span>
            <Input
              type="password"
              autoComplete="off"
              value={props.tokenDraft}
              placeholder="Paste the app token"
              aria-label="Thingiverse app token"
              onChange={(event) => props.onTokenDraft(event.target.value)}
            />
          </label>
          <div className="models-actions">
            <Button type="button" variant="primary" onClick={props.onSaveToken}>
              Save token
            </Button>
            <Button type="button" variant="ghost" onClick={() => props.onOpenListing(THINGIVERSE_DEVELOPERS)}>
              Open the developer page
            </Button>
          </div>
        </section>
      ) : (
        <>
          <form
            className="models-search"
            onSubmit={(event) => {
              event.preventDefault();
              props.onSearch();
            }}
          >
            <label className="models-search__field">
              <span className="sr-only">Search models</span>
              <Search size={16} strokeWidth={1.5} aria-hidden />
              <Input
                value={props.query}
                placeholder={props.library === "makerworld" ? "Search MakerWorld" : "Search Thingiverse"}
                aria-label="Search models"
                onChange={(event) => props.onQuery(event.target.value)}
              />
            </label>
            <Select label="Sort" value={props.sort} options={sortsFor(props.library)} onChange={props.onSort} />
            <Button type="submit" variant="primary" disabled={props.searching}>
              {props.searching ? "Searching…" : "Search"}
            </Button>
          </form>

          <form
            className="models-link"
            onSubmit={(event) => {
              event.preventDefault();
              props.onOpenLink();
            }}
          >
            <Input
              value={props.link}
              placeholder="Or paste a MakerWorld or Thingiverse link"
              aria-label="Model link"
              onChange={(event) => props.onLink(event.target.value)}
            />
            <Button type="submit" variant="secondary" disabled={props.opening || props.link.trim() === ""}>
              Open in Shale
            </Button>
          </form>

          {props.library === "thingiverse" ? (
            <p className="models-token-line">
              Thingiverse token saved.{" "}
              <button type="button" className="models-text-button" onClick={props.onClearToken}>
                Remove token
              </button>
            </p>
          ) : null}

          {props.error ? (
            <div className="models-banner" role="alert">
              <p>{props.error}</p>
              {props.error.toLowerCase().includes("bambu lab account") ? (
                <Button type="button" variant="secondary" onClick={props.onOpenAccounts}>
                  Sign in
                </Button>
              ) : null}
            </div>
          ) : null}

          <div className="models-layout">
            <div className="models-results">
              {props.searching && props.hits.length === 0 ? (
                <div className="ui-sidebar__skeleton" aria-hidden>
                  <i style={{ width: "72%" }} />
                  <i style={{ width: "54%" }} />
                  <i style={{ width: "63%" }} />
                </div>
              ) : props.hits.length === 0 ? (
                <p className="models-hint">Search by name, or paste a link to a model page.</p>
              ) : (
                <ul className="models-grid">
                  {props.hits.map((hit) => (
                    <li key={`${hit.library}-${hit.id}`}>
                      <button
                        type="button"
                        className={props.listingId === hit.id ? "models-card is-on" : "models-card"}
                        onClick={() => props.onSelect(hit.id)}
                      >
                        <span
                          className="models-thumb"
                          style={{ background: `linear-gradient(145deg, hsl(${hue(hit.title)} 42% 42%), hsl(${hue(hit.creator)} 28% 18%))` }}
                        >
                          {hit.thumbnail_url ? <img src={hit.thumbnail_url} alt="" /> : <span>{hit.title.slice(0, 1)}</span>}
                        </span>
                        <span className="models-card__body">
                          <span className="models-card__title">{hit.title}</span>
                          <span className="models-card__by">By {hit.creator}</span>
                          <span className="models-card__licence">{hit.licence}</span>
                          {hit.nsfw ? <span className="models-chip">Marked sensitive</span> : null}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {props.hits.length > 0 ? (
                <div className="models-pager">
                  <Button type="button" variant="ghost" size="sm" disabled={props.page <= 1 || props.searching} onClick={() => props.onPage(props.page - 1)}>
                    Previous
                  </Button>
                  <span>
                    Page {props.page} of {pages}
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={props.page >= pages || props.searching}
                    onClick={() => props.onPage(props.page + 1)}
                  >
                    Next
                  </Button>
                </div>
              ) : null}
            </div>

            <aside className="models-detail" aria-live="polite">
              {selected ? (
                <>
                  <h2>{selected.card.title}</h2>
                  <p className="models-detail__by">
                    By {selected.card.creator}
                    {selected.card.creator_url ? (
                      <>
                        {" "}
                        <button type="button" className="models-text-button" onClick={() => props.onOpenListing(selected.card.creator_url!)}>
                          Creator page
                        </button>
                      </>
                    ) : null}
                  </p>
                  <p className="models-detail__licence">
                    <span>Licence</span>
                    {selected.card.licence}
                  </p>
                  <p className="models-detail__summary">
                    {selected.summary.trim() || "No description was published with this model."}
                  </p>
                  {selected.profiles.length > 0 ? (
                    <fieldset className="models-choices">
                      <legend>Print profiles</legend>
                      {selected.profiles.map((profile) => (
                        <label key={profile.id}>
                          <input
                            type="radio"
                            name="print-profile"
                            checked={props.profileId === profile.id}
                            onChange={() => props.onProfile(profile.id)}
                          />
                          <span>
                            {profile.name}
                            {profile.printer ? <small>{profile.printer}</small> : null}
                          </span>
                        </label>
                      ))}
                    </fieldset>
                  ) : null}
                  {selected.files.length > 0 ? (
                    <fieldset className="models-choices">
                      <legend>Files</legend>
                      {selected.files.map((file) => (
                        <label key={file.id}>
                          <input
                            type="radio"
                            name="library-file"
                            checked={props.fileId === file.id}
                            onChange={() => props.onFile(file.id)}
                          />
                          <span>
                            {file.name}
                            <small>{file.kind.toUpperCase()}</small>
                          </span>
                        </label>
                      ))}
                    </fieldset>
                  ) : null}
                  <div className="models-actions">
                    <Button type="button" variant="primary" disabled={props.opening} onClick={props.onOpen}>
                      {props.opening ? "Opening…" : "Open in Shale"}
                    </Button>
                    <Button type="button" variant="ghost" onClick={() => props.onOpenListing(selected.card.page_url)}>
                      <ExternalLink size={14} strokeWidth={1.5} />
                      View listing
                    </Button>
                  </div>
                </>
              ) : (
                <p className="models-hint">Choose a model to see the creator, the licence, and its print profiles.</p>
              )}
            </aside>
          </div>
        </>
      )}
    </div>
  );
}

export function ModelsView({
  onOpened,
  onOpenAccounts,
}: {
  onOpened: (saved: SavedModel) => Promise<void> | void;
  onOpenAccounts: () => void;
}) {
  const [library, setLibrary] = useState<ModelLibrary>("makerworld");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("relevant");
  const [page, setPage] = useState(1);
  const [hits, setHits] = useState<ModelCard[]>([]);
  const [total, setTotal] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [listing, setListing] = useState<ModelListing | null>(null);
  const [listingId, setListingId] = useState<string | null>(null);
  const [profileId, setProfileId] = useState<string | null>(null);
  const [fileId, setFileId] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const [link, setLink] = useState("");
  const [tokenConfigured, setTokenConfigured] = useState(false);
  const [tokenDraft, setTokenDraft] = useState("");

  useEffect(() => {
    let live = true;
    api
      .thingiverseTokenStatus()
      .then((status) => {
        if (live) setTokenConfigured(status.configured);
      })
      .catch(() => {
        if (live) setTokenConfigured(false);
      });
    return () => {
      live = false;
    };
  }, []);

  function chooseLibrary(next: ModelLibrary) {
    setLibrary(next);
    setHits([]);
    setTotal(0);
    setPage(1);
    setListing(null);
    setListingId(null);
    setError(null);
    const allowed = next === "thingiverse" ? ["relevant", "popular", "newest"] : ["relevant", "popular", "downloads"];
    if (!allowed.includes(sort)) setSort("relevant");
  }

  async function search(nextPage = page) {
    const text = query.trim();
    if (!text) {
      setError("Say what to search for.");
      return;
    }
    setSearching(true);
    setError(null);
    try {
      const result = await api.searchModels(text, library, nextPage, sort);
      setHits(result.hits);
      setTotal(result.total);
      setPage(nextPage);
      if (result.hits.length === 0) setError("No models matched that search.");
    } catch (err) {
      setHits([]);
      setError(messageOf(err));
    } finally {
      setSearching(false);
    }
  }

  async function select(id: string) {
    setListingId(id);
    setError(null);
    try {
      const next = await api.modelDetail(library, id);
      setListing(next);
      setProfileId(next.profiles[0]?.id ?? null);
      const preferred = next.files.find((file) => file.kind === "3mf") ?? next.files[0];
      setFileId(preferred?.id ?? null);
    } catch (err) {
      setListing(null);
      setError(messageOf(err));
    }
  }

  async function openSelected() {
    if (!listingId) return;
    setOpening(true);
    setError(null);
    try {
      const saved = await api.downloadModel(library, listingId, profileId, fileId);
      await onOpened(saved);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setOpening(false);
    }
  }

  async function openPasted() {
    const url = link.trim();
    if (!url) return;
    setOpening(true);
    setError(null);
    try {
      const saved = await api.importModelUrl(url);
      await onOpened(saved);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setOpening(false);
    }
  }

  async function saveToken() {
    setError(null);
    try {
      const status = await api.thingiverseTokenSet(tokenDraft);
      setTokenConfigured(status.configured);
      setTokenDraft("");
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function clearToken() {
    setError(null);
    try {
      const status = await api.thingiverseTokenClear();
      setTokenConfigured(status.configured);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  return (
    <ModelsDiscover
      library={library}
      onLibrary={chooseLibrary}
      query={query}
      onQuery={setQuery}
      sort={sort}
      onSort={setSort}
      onSearch={() => void search(1)}
      searching={searching}
      error={error}
      hits={hits}
      total={total}
      page={page}
      onPage={(next) => void search(next)}
      listing={listing}
      listingId={listingId}
      onSelect={(id) => void select(id)}
      profileId={profileId}
      onProfile={setProfileId}
      fileId={fileId}
      onFile={setFileId}
      onOpen={() => void openSelected()}
      opening={opening}
      link={link}
      onLink={setLink}
      onOpenLink={() => void openPasted()}
      tokenConfigured={tokenConfigured}
      tokenDraft={tokenDraft}
      onTokenDraft={setTokenDraft}
      onSaveToken={() => void saveToken()}
      onClearToken={() => void clearToken()}
      onOpenAccounts={onOpenAccounts}
      onOpenListing={(url) => {
        void api.openModelLink(url).catch((err) => {
          setError(messageOf(err));
        });
      }}
    />
  );
}
