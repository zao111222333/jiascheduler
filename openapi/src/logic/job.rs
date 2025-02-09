use anyhow::{anyhow, Result};

mod bundle_script;
mod dashboard;
mod exec_history;
mod schedule;
mod timer;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, Select, Set,
};
use sea_query::Expr;

use crate::{
    entity::{
        self, executor, job, job_exec_history, job_running_status, job_schedule_history, prelude::*,
    },
    state::AppContext,
};
use sea_orm::JoinType;

pub mod types;

pub struct JobLogic<'a> {
    ctx: &'a AppContext,
}

impl<'a> JobLogic<'a> {
    pub fn new(ctx: &'a AppContext) -> Self {
        Self { ctx }
    }
    pub async fn save_job(
        &self,
        model: entity::job::ActiveModel,
    ) -> Result<entity::job::ActiveModel> {
        let model = model.save(&self.ctx.db).await?;
        Ok(model)
    }

    pub async fn query_job(
        &self,
        created_user: Option<String>,
        job_type: Option<String>,
        name: Option<String>,
        updated_time_range: Option<(String, String)>,
        default_id: Option<u64>,
        default_eid: Option<String>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<types::JobRelatedExecutorModel>, u64)> {
        let model = Job::find()
            .column_as(executor::Column::Name, "executor_name")
            .column_as(executor::Column::Command, "executor_command")
            .join_rev(
                JoinType::LeftJoin,
                Executor::belongs_to(Job)
                    .from(executor::Column::Id)
                    .to(job::Column::ExecutorId)
                    .into(),
            )
            .apply_if(created_user, |query, v| {
                query.filter(job::Column::CreatedUser.eq(v))
            })
            .apply_if(job_type, |query, v| {
                query.filter(job::Column::JobType.eq(v))
            })
            .apply_if(name, |query, v| query.filter(job::Column::Name.contains(v)))
            .apply_if(updated_time_range, |query, v| {
                query.filter(
                    job::Column::UpdatedTime
                        .gt(v.0)
                        .and(job::Column::UpdatedTime.lt(v.1)),
                )
            });

        let total = model.clone().count(&self.ctx.db).await?;
        let list = model
            .apply_if(default_id, |query, v| {
                query.order_by_desc(Expr::expr(job::Column::Id.eq(v)))
            })
            .apply_if(default_eid, |query, v| {
                query.order_by_desc(Expr::expr(job::Column::Eid.eq(v)))
            })
            .order_by_desc(entity::job::Column::Id)
            .into_model()
            .paginate(&self.ctx.db, page_size)
            .fetch_page(page)
            .await?;
        Ok((list, total))
    }

    pub async fn delete_job(&self, eid: String) -> Result<u64> {
        let record = JobExecHistory::find()
            .filter(job_exec_history::Column::Eid.eq(&eid))
            .one(&self.ctx.db)
            .await?;
        if record.is_some() {
            anyhow::bail!("forbidden to delete the executed jobs")
        }

        let ret = Job::delete(entity::job::ActiveModel {
            eid: Set(eid),
            ..Default::default()
        })
        .exec(&self.ctx.db)
        .await?;
        Ok(ret.rows_affected)
    }

    #[allow(unused)]
    async fn query_list<T>(
        &self,
        query: Select<T>,
        created_user: Option<String>,
        ip: Option<String>,
        schedule_name: Option<String>,
        schedule_type: Option<String>,
        job_type: Option<String>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<types::RunStatusRelatedScheduleJobModel>, u64)>
    where
        T: EntityTrait,
        T: FromQueryResult + Sized + Send + Sync + 'a,
    {
        let model = query
            .column_as(job_schedule_history::Column::Name, "schedule_name")
            .column_as(
                job_schedule_history::Column::SnapshotData,
                "schedule_snapshot_data",
            )
            .join_rev(
                JoinType::LeftJoin,
                JobScheduleHistory::belongs_to(JobRunningStatus)
                    .from(job_schedule_history::Column::ScheduleId)
                    .to(job_running_status::Column::ScheduleId)
                    .into(),
            )
            .apply_if(created_user, |query, v| {
                query.filter(job_running_status::Column::UpdatedUser.eq(v))
            })
            .apply_if(ip, |query, v| {
                query.filter(job_running_status::Column::BindIp.contains(v))
            })
            .apply_if(schedule_name, |query, v| {
                query.filter(job_schedule_history::Column::Name.contains(v))
            });

        let total = model
            .clone()
            .select_only()
            .column_as(job_running_status::Column::Id.count(), "count")
            .into_tuple()
            .one(&self.ctx.db)
            .await?
            .ok_or(anyhow!("failed to count row"))?;

        let list = model
            .order_by_desc(entity::job_running_status::Column::UpdatedTime)
            .into_model()
            .paginate(&self.ctx.db, page_size)
            .fetch_page(page)
            .await?;
        Ok((list, total))
    }

    pub async fn query_run_list(
        &self,
        created_user: Option<String>,
        bind_ip: Option<String>,
        schedule_name: Option<String>,
        schedule_type: Option<String>,
        job_type: Option<String>,
        updated_time_range: Option<(String, String)>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<types::RunStatusRelatedScheduleJobModel>, u64)> {
        let model = JobRunningStatus::find()
            .column_as(job_schedule_history::Column::Name, "schedule_name")
            .column_as(
                job_schedule_history::Column::SnapshotData,
                "schedule_snapshot_data",
            )
            .join_rev(
                JoinType::LeftJoin,
                JobScheduleHistory::belongs_to(JobRunningStatus)
                    .from(job_schedule_history::Column::ScheduleId)
                    .to(job_running_status::Column::ScheduleId)
                    .into(),
            )
            .apply_if(schedule_type, |query, v| {
                query.filter(entity::job_running_status::Column::ScheduleType.eq(v))
            })
            .apply_if(job_type, |query, v| {
                query.filter(entity::job_running_status::Column::JobType.eq(v))
            })
            .apply_if(created_user, |query, v| {
                query.filter(entity::job_running_status::Column::UpdatedUser.eq(v))
            })
            .apply_if(bind_ip, |query, v| {
                query.filter(entity::job_running_status::Column::BindIp.contains(v))
            })
            .apply_if(schedule_name, |query, v| {
                query.filter(job_schedule_history::Column::Name.contains(v))
            })
            .apply_if(updated_time_range, |query, v| {
                query.filter(
                    job_running_status::Column::UpdatedTime
                        .gt(v.0)
                        .and(job::Column::UpdatedTime.lt(v.1)),
                )
            });

        let total = model.clone().count(&self.ctx.db).await?;

        let list = model
            .order_by_desc(entity::job_running_status::Column::UpdatedTime)
            .into_model()
            .paginate(&self.ctx.db, page_size)
            .fetch_page(page)
            .await?;
        Ok((list, total))
    }
}
