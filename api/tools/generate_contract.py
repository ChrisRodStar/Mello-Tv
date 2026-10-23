#!/usr/bin/env python3
"""Generate the committed public api.v1 OpenAPI contract, without credentials."""
import json
from pathlib import Path

def obj(props, required=None):
    return {"type":"object","properties":props,"required":list(props) if required is None else required,"additionalProperties":False}
def ref(name): return {"$ref":"#/components/schemas/"+name}
def array(item): return {"type":"array","items":item}
def enum(*v): return {"type":"string","enum":list(v)}
string={"type":"string"}
integer={"type":"integer"}
positive={"type":"integer","minimum":1}
nonnegative={"type":"number","minimum":0}
boolean={"type":"boolean"}
timestamp={"type":"string","format":"date-time"}
identity={"type":enum("movie","tv"),"id":positive}
target=dict(identity,season={"type":"integer","minimum":0},episode=positive)
S={}
S["Error"]=obj({"error":string})
S["Identity"]=obj(identity)
S["Target"]=obj(target,["type","id"])
S["Preferences"]=obj({"completed":boolean,"format":enum("both","movie","tv"),"genres":array(enum("action","comedy","drama","crime","scifi","animation","documentary","horror","family"))})
S["ProfileEdit"]=obj({"name":{"type":"string","minLength":1,"maxLength":80},"color":{"type":"string","minLength":1,"maxLength":32}})
S["Profile"]=obj(dict(S["ProfileEdit"]["properties"],id=string,preferences=ref("Preferences")))
S["Device"]=obj({"formats":array(enum("hls","mp4")),"codecs":array(string),"max_height":{"type":"integer","minimum":0},"allow_unknown_codecs":boolean,"allow_unknown_resolution":boolean,"supports_encrypted_hls":boolean},["formats"])
S["PlaybackPreferences"]=obj({"audio_languages":array(string),"subtitle_languages":array(string),"require_audio":boolean,"require_subtitles":boolean,"max_height":{"type":"integer","minimum":0}},[])
S["PlaybackRequest"]=obj(dict(target,imdb_id={"type":"string","pattern":"^tt[0-9]{1,20}$"},mal_id=positive,device=ref("Device"),preferences=ref("PlaybackPreferences")),["type","id"])
S["PlaybackReply"]=obj({"id":string,"title":ref("Target"),"url":{"type":"string","pattern":"^/stream/[A-Za-z0-9_-]+/entry\\?st="},"format":enum("hls","mp4"),"expires_at":timestamp,"revision":positive,"source_generation":positive,"reload_required":boolean,"resume_seconds":nonnegative})
S["PlaybackReply"]["properties"]["available_heights"]={"type":["array","null"],"items":{"type":"integer","minimum":1,"maximum":16384},"maxItems":32,"uniqueItems":True}
S["ProgressUpdate"]=obj({"playback_id":string,"sequence":positive,"position_seconds":nonnegative,"duration_seconds":nonnegative,"watched_seconds":nonnegative})
S["Progress"]=obj(dict(target,position_seconds=nonnegative,duration_seconds=nonnegative,watched_seconds=nonnegative,completed=boolean,updated_at=timestamp),["type","id","position_seconds","duration_seconds","watched_seconds","completed","updated_at"])
S["SeasonSummary"]=obj({"season":{"type":"integer","minimum":0},"name":string,"episode_count":{"type":"integer","minimum":0}})
S["Title"]=obj(dict(identity,title=string,overview=string,date=string,rating={"type":"number"},poster=string,backdrop=string,imdb_id=string,mal_id=positive,runtime={"type":"integer","minimum":0,"description":"Minutes, deliberately distinct from public playback/progress seconds."},genres=array(string),seasons=array(ref("SeasonSummary")),details=boolean),["type","id","title","overview","rating","details"])
S["Episode"]=obj({"tmdb_episode_id":integer,"season":{"type":"integer","minimum":0},"episode":positive,"name":string,"overview":string,"runtime":{"type":"integer","description":"Minutes"},"still":string,"air_date":string},["tmdb_episode_id","season","episode","name","overview","runtime"])
S["Season"]=obj({"show_id":positive,"season":{"type":"integer","minimum":0},"name":string,"episodes":array(ref("Episode"))})
S["Shelf"]=obj({"id":string,"heading":string,"titles":array(ref("Title")),"kind":string,"source":string,"reason":string},["id","heading","titles"])
S["Recommendations"]=obj({"fingerprint":string,"pool":array(ref("Title")),"sections":array(ref("Shelf"))})
record={"kind":enum("title","season","shelf","search","recommendations"),"key":string,"data":{"oneOf":[ref("Title"),ref("Season"),ref("Shelf"),ref("Recommendations"),array(ref("Title"))]},"updated_at":timestamp,"expires_at":timestamp,"revision":positive,"state":enum("fresh","stale")}
S["Record"]=obj(record)
S["EpisodeReply"]=obj({"show_id":positive,"data":ref("Episode"),"state":enum("fresh","stale"),"updated_at":timestamp,"expires_at":timestamp})
S["Mutation"]=obj({"expected_revision":positive})
S["Status"]=obj({"version":{"const":"api.v1"},"catalog_snapshot":enum("missing","retained"),"catalog_revision":integer,"exported_at":timestamp,"cached_records":integer,"pending_revocations":integer,"pending_profile_deletions":integer})
paths={}
def op(method,path,summary,response=None,body=None,status=200,auth=True,idem=False,query=None):
    parameters=[]
    for part in path.split("/"):
        if part.startswith("{"):
            name=part[1:-1];schema=string
            if name in ("id","episode"):schema=positive
            if name=="season":schema={"type":"integer","minimum":0}
            if name=="type":schema=enum("movie","tv")
            if name=="topic":schema={"type":"string","pattern":"^(topic-|taste-)[A-Za-z0-9_-]+$"}
            parameters.append({"name":name,"in":"path","required":True,"schema":schema})
    if idem: parameters.append({"name":"Idempotency-Key","in":"header","required":True,"schema":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"}})
    for name,schema,required in query or []:parameters.append({"name":name,"in":"query","required":required,"schema":schema})
    operation={"summary":summary,"operationId":method+"_"+path.strip("/").replace("/","_").replace("{","").replace("}",""),"security":[],"responses":{str(status):{"description":"Success"},"default":{"description":"Stable redacted error","content":{"application/json":{"schema":ref("Error")}}}}}
    if response is not None:operation["responses"][str(status)]["content"]={"application/json":{"schema":response}}
    if parameters:operation["parameters"]=parameters
    if body:operation["requestBody"]={"required":True,"content":{"application/json":{"schema":body}}}
    paths.setdefault(path,{})[method]=operation
base="/api/v1"
op("get",base+"/profiles","List owned profiles",obj({"profiles":array(ref("Profile"))}))
op("post",base+"/profiles","Create owned profile",ref("Profile"),ref("ProfileEdit"),201)
p=base+"/profiles/{profile}"
op("get",p,"Read owned profile",ref("Profile"));op("put",p,"Edit name/color",ref("Profile"),ref("ProfileEdit"));op("delete",p,"Delete personal state and persist private-service cleanup",status=204)
op("put",p+"/preferences","Persist onboarding and recommendation preferences",ref("Profile"),ref("Preferences"))
op("post",p+"/activity","Record today's UTC activity",status=204)
for kind in ("library","favorites"):
    op("get",p+"/"+kind,"Read saved titles",obj({"titles":array(ref("Title"))}))
    for method in ("put","delete"):op(method,p+"/"+kind+"/{type}/{id}","Save/remove known title",status=204)
offset=("offset",{"type":"integer","minimum":0,"maximum":20000},False)
op("get",p+"/history","Read persistent viewing history",obj({"records":array(ref("Progress")),"offset":integer,"limit":{"const":100}}),query=[offset])
op("get",p+"/resume","Targeted exact title/episode resume lookup",ref("Progress"),query=[("type",enum("movie","tv"),True),("id",positive,True),("season",{"type":"integer","minimum":0},False),("episode",positive,False)])
op("put",p+"/progress","Update cumulative measured viewing idempotently",ref("Progress"),ref("ProgressUpdate"))
op("get",p+"/home","Assembled recommendation page from saved catalog data and current profile choices",obj({"shelves":array(ref("Record")),"recommendations":{"oneOf":[ref("Record"),{"type":"null"}]},"sections":array(ref("Shelf")),"featured":{"type":"array","maxItems":1,"items":ref("Title")},"page_version":{"type":"string","description":"Stable content hash for the assembled page."}}),query=[("type",dict(enum("all","movie","tv"),default="all"),False),("topic",dict(enum("all","short","classics","discovery"),default="all"),False),("shelf",{"type":"string","maxLength":200,"description":"See all: one known category, up to 100 eligible saved titles, without a featured reservation."},False)])
op("get",p+"/recommendations","Owned cached recommendation view",ref("Record"),query=[("type",enum("all","movie","tv"),False)])
op("post",p+"/playback","Resolve and import one provider Selection",ref("PlaybackReply"),ref("PlaybackRequest"),idem=True)
op("get",p+"/playback/{playback}","Reconcile persisted playback and issue grant",ref("PlaybackReply"))
op("delete",p+"/playback/{playback}","Persist revocation and revoke relay session",status=204)
for action in ("renew","backup","refresh"):op("post",p+"/playback/{playback}/"+action,"Explicit playback "+action,ref("PlaybackReply"),ref("Mutation"),idem=True)
op("get",base+"/browse","Retained general shelves",obj({"records":array(ref("Record"))}))
op("get",base+"/search","Search cached titles",obj({"scope":{"const":"cached_titles"},"records":array(ref("Record")),"offset":integer,"limit":{"const":50}}),query=[("q",{"type":"string","minLength":1,"maxLength":200},True),offset])
op("get",base+"/search-results","Retained exact-query discovery result",ref("Record"),query=[("q",string,True)])
for suffix in ("/titles/{type}/{id}","/tv/{id}/seasons/{season}"):op("get",base+suffix,"Retained title/season envelope",ref("Record"))
op("get",base+"/tv/{id}/seasons/{season}/episodes/{episode}","Exact retained episode",ref("EpisodeReply"))
for method in ("get","head"):
    op(method,base+"/artwork/{size}/{filename}","Legacy artwork reference redirects to TMDB image CDN",status=302)
    paths[base+"/artwork/{size}/{filename}"][method]["responses"]["302"]["headers"]={"Location":{"schema":{"type":"string","format":"uri"},"description":"Public image.tmdb.org URL. Image payloads are fetched and cached by the client."}}
op("get",base+"/status","Redacted retained-state diagnostics",ref("Status"))
for health in ("livez","readyz"):op("get","/health/"+health,"API lifecycle/storage health",obj({"version":{"const":"api.v1"},"status":string}),auth=False)
for kind in ("hidden","watched","ignored"):
    path=p+"/recommendation-"+kind
    op("get",path,"Read "+kind+" recommendation choices",obj({"titles":array(ref("Title"))}))
    for method in ("put","delete"):
        op(method,path+"/{type}/{id}","Save or undo "+kind+" recommendation choices",status=204)
feedback={kind:array(ref("Title")) for kind in ("hidden","watched","ignored","favorites")}
feedback["topics"]=array(string)
op("get",p+"/recommendation-feedback","Read reversible recommendation choices",obj(feedback))
op("get",p+"/recommendation-topics","Read hidden category IDs",obj({"topics":array(string)}))
op("put",p+"/recommendation-topics/{topic}","Hide a known category",status=204)
op("delete",p+"/recommendation-topics/{topic}","Restore a category",status=204)
doc={"openapi":"3.1.0","info":{"title":"Stream tv public API","version":"api.v1","description":"Private-network household access with no account login or password. Profiles share one retained household. All public progress/playback times use seconds; catalog runtime remains minutes. See docs/api.md for ownership, required TV coordinates, limits and retry/recovery semantics."},"paths":paths,"components":{"schemas":S}}
(Path(__file__).resolve().parents[1]/"contracts/openapi.json").write_text(json.dumps(doc,indent=2)+"\n")
